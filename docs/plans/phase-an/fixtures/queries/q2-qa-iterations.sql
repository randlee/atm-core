-- Q2: distinct QA rounds per sprint.
SELECT
    json_extract(vars_json, '$.sprint') AS sprint,
    COUNT(DISTINCT json_extract(vars_json, '$.round')) AS qa_iterations
FROM decomposed_messages
WHERE category = 'qa-finding'
  AND json_extract(vars_json, '$.sprint') IS NOT NULL
  AND json_extract(vars_json, '$.round') IS NOT NULL
GROUP BY sprint
ORDER BY sprint;
