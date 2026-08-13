-- Template-agnostic analogue: a corpus with no ATM workflow vocabulary.
-- It uses the same public view and JSON surface as Q1-Q4.
SELECT
    json_extract(vars_json, '$.cycle') AS cycle,
    MIN(CASE WHEN category = 'opened' THEN message_at END) AS opened_at,
    MAX(CASE WHEN category = 'delivered' THEN message_at END) AS delivered_at,
    MAX(CASE WHEN category = 'opened' THEN json_extract(vars_json, '$.owner') END) AS owner,
    SUM(CASE WHEN category = 'assessment'
              AND json_extract(vars_json, '$.risk') = 'high' THEN 1 ELSE 0 END) AS high_risk_assessments
FROM decomposed_messages
WHERE team = 'fixture-synthetic'
GROUP BY cycle;
