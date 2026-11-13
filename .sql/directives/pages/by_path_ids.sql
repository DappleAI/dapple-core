args AS (
    SELECT *
    FROM UNNEST($1::UUID[]) WITH ORDINALITY AS o(path_id, ord)
),
directive_page AS (
    SELECT d.id
    FROM args
    JOIN rev.object_paths op ON op.id = args.path_id
    JOIN rev.directive_path_details dpd ON dpd.path_id = op.id
    JOIN rev.versioned_objects vo ON vo.path_id = op.id
    JOIN rev.directives d ON d.versioned_object_id = vo.id
)
