use serde::Serialize;

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum HelpTopicTier {
    Tier1,
    Tier2,
}

impl HelpTopicTier {
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Tier1 => "tier 1",
            Self::Tier2 => "tier 2",
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum HelpResultKind {
    Overview,
    TopicList,
    ConceptTopic,
    CommandHelp,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub(crate) struct HelpTopicSummary {
    pub(crate) name: &'static str,
    pub(crate) tier: HelpTopicTier,
    pub(crate) summary: &'static str,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub(crate) struct HelpResult {
    pub(crate) kind: HelpResultKind,
    pub(crate) requested_target: Option<String>,
    pub(crate) title: String,
    pub(crate) body: String,
    pub(crate) commands: Vec<String>,
    pub(crate) topics: Vec<HelpTopicSummary>,
}
