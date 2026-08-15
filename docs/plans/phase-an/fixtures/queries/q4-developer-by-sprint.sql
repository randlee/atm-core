-- Q4: development agent recorded on the assignment for each sprint.
SELECT
    json_extract(vars_json, '$.sprint') AS sprint,
    agent AS developer
FROM decomposed_messages
WHERE category = 'assignment'
  AND json_extract(vars_json, '$.sprint') IS NOT NULL
ORDER BY sprint, message_at;
