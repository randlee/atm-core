use super::{
    MAX_RECONCILE_FINGERPRINT_KEYS, MAX_RECONCILE_FINGERPRINTS_PER_KEY, ReconcileKey,
    ReconcileRequest, ReconcileWorkerState,
};
use std::collections::{HashMap, HashSet, VecDeque};

#[derive(Default)]
pub(super) struct NotificationFingerprintRegistry {
    pub(super) entries: HashMap<ReconcileKey, HashSet<NotificationFingerprint>>,
    pub(super) order: VecDeque<ReconcileKey>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub(super) struct NotificationFingerprint(pub(super) String);

impl NotificationFingerprint {
    pub(super) fn new(value: String) -> Option<Self> {
        if value.is_empty() {
            None
        } else {
            Some(Self(value))
        }
    }
}

pub(super) fn should_emit_reconcile_notification(
    worker_state: &mut ReconcileWorkerState,
    request: &ReconcileRequest,
    current_fingerprints: Option<HashSet<NotificationFingerprint>>,
) -> bool {
    let Some(mut current_fingerprints) = current_fingerprints else {
        return true;
    };

    let key = ReconcileKey::from_request(request);
    if current_fingerprints.len() > MAX_RECONCILE_FINGERPRINTS_PER_KEY {
        let mut ordered = current_fingerprints.drain().collect::<Vec<_>>();
        ordered.sort();
        let dropped = ordered
            .len()
            .saturating_sub(MAX_RECONCILE_FINGERPRINTS_PER_KEY);
        ordered.truncate(MAX_RECONCILE_FINGERPRINTS_PER_KEY);
        current_fingerprints.extend(ordered);
        tracing::warn!(
            subsystem = "reconcile",
            action = "fingerprint_truncate",
            outcome = "cap_exceeded",
            team = %key.team,
            agent = %key.agent,
            retained = MAX_RECONCILE_FINGERPRINTS_PER_KEY,
            dropped,
            "reconcile notification fingerprint set exceeded the per-key bounded cap; truncating deterministically"
        );
    }

    let fingerprints = &mut worker_state.notification_fingerprints;
    let changed = fingerprints
        .entries
        .get(&key)
        .map(|previous| previous != &current_fingerprints)
        .unwrap_or(true);
    let is_new_key = !fingerprints.entries.contains_key(&key);
    if is_new_key && fingerprints.entries.len() >= MAX_RECONCILE_FINGERPRINT_KEYS {
        while let Some(evicted_key) = fingerprints.order.pop_front() {
            if fingerprints.entries.remove(&evicted_key).is_some() {
                tracing::warn!(
                    subsystem = "reconcile",
                    action = "fingerprint_evict",
                    outcome = "cap_exceeded",
                    team = %evicted_key.team,
                    agent = %evicted_key.agent,
                    cap = MAX_RECONCILE_FINGERPRINT_KEYS,
                    "evicted oldest reconcile notification fingerprint entry after reaching the bounded cap"
                );
                break;
            }
        }
    }
    if is_new_key {
        fingerprints.order.push_back(key.clone());
    }
    fingerprints.entries.insert(key, current_fingerprints);
    changed
}
