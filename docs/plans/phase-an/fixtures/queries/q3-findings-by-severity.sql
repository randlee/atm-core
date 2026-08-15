-- Q3: QA findings by sprint, round, and severity.
SELECT
    json_extract(vars_json, '$.sprint') AS sprint,
    json_extract(vars_json, '$.round') AS qa_round,
    json_extract(vars_json, '$.severity') AS severity,
    COUNT(*) AS findings
FROM decomposed_messages
WHERE category = 'qa-finding'
  AND json_extract(vars_json, '$.sprint') IS NOT NULL
  AND json_extract(vars_json, '$.round') IS NOT NULL
  AND json_extract(vars_json, '$.severity') IN ('Blocking', 'Important', 'Minor')
GROUP BY sprint, qa_round, severity
ORDER BY sprint, qa_round, severity;
