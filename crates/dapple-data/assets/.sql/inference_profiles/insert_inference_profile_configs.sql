WITH args AS (
    SELECT *
    FROM UNNEST(
        $1::UUID[],           $2::UUID[],            $3::UUID[],            $4::UUID[],            $5::INT[]          ) WITH ORDINALITY AS o(
        inference_profile_id,
        prompt_path_object_id,
        model_release_id,
        model_variant_id,
        priority,
        ord
    )
),
_inserted AS (
    INSERT INTO rev.inference_profile_configs (
        inference_profile_id,
        prompt_path_object_id,
        model_release_id,
        model_variant_id,
        priority
    )
    SELECT
        args.inference_profile_id,
        args.prompt_path_object_id,
        args.model_release_id,
        args.model_variant_id,
        args.priority
    FROM args
    WHERE NOT EXISTS (
        SELECT 1 FROM rev.inference_profile_configs ipc
        WHERE ipc.inference_profile_id = args.inference_profile_id
          AND ipc.prompt_path_object_id = args.prompt_path_object_id
    )
    ORDER BY args.ord
    RETURNING id
)
SELECT
    ipc.id AS "id!",
    ipc.inference_profile_id AS "inference_profile_id!",
    ipc.prompt_path_object_id AS "prompt_path_id!",
    ipc.model_release_id,
    ipc.model_variant_id,
    ipc.priority AS "priority!"
FROM args
JOIN rev.inference_profile_configs ipc
    ON ipc.inference_profile_id = args.inference_profile_id
    AND ipc.prompt_path_object_id = args.prompt_path_object_id
ORDER BY ipc.id;
