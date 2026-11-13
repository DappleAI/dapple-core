-- Insert inference profile configs (two-phase: after profile creation)
-- Maps prompts to model releases/variants with priority ordering
WITH args AS (
    SELECT *
    FROM UNNEST(
        $1::UUID[],   -- inference_profile_id[]
        $2::UUID[],    -- prompt_path_object_id[]
        $3::UUID[],    -- model_release_id[] (nullable)
        $4::UUID[],    -- model_variant_id[] (nullable)
        $5::INT[]      -- priority[]
    ) WITH ORDINALITY AS o(
        inference_profile_id,
        prompt_path_object_id,
        model_release_id,
        model_variant_id,
        priority,
        ord
    )
),
new_configs AS (
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
    ORDER BY args.ord
    RETURNING id, inference_profile_id, prompt_path_object_id AS prompt_path_id, model_release_id, model_variant_id, priority
)
SELECT
    nc.id AS "id!",
    nc.inference_profile_id AS "inference_profile_id!",
    nc.prompt_path_id AS "prompt_path_id!",
    nc.model_release_id,
    nc.model_variant_id,
    nc.priority AS "priority!"
FROM new_configs nc
ORDER BY nc.id;
