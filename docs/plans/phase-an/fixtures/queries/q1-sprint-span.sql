-- Q1: first assignment through completion for every sprint.
SELECT
    json_extract(vars_json, '$.sprint') AS sprint,
    MIN(CASE WHEN category = 'assignment' THEN message_at END) AS first_assignment_at,
    MAX(CASE WHEN category = 'completion' THEN message_at END) AS completion_at
FROM decomposed_messages
WHERE category IN ('assignment', 'completion')
  AND json_extract(vars_json, '$.sprint') IS NOT NULL
GROUP BY sprint
ORDER BY sprint;
