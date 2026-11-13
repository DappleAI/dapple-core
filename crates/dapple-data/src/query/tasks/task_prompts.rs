use crate::DbPool;
use crate::error::DataError;
use crate::model::tasks::task_prompts::InsertTaskPromptsArgs;
use crate::model::tasks::task_prompts::InsertedTaskPromptModel;
use crate::model::tasks::task_prompts::TaskPrompt;
use crate::model::tasks::task_prompts::TaskPromptModel;
use log::debug;
use sqlx::types::Uuid;
use sqlx::{Error, Postgres, Transaction};

/// Insert task_prompts using IDs (resolved in SQL via CTE)
/// Note: prompt_name is derived from prompt_paths.name via SQL join
pub async fn insert_task_prompt_models(
    tx: &mut Transaction<'static, Postgres>,
    args: InsertTaskPromptsArgs,
) -> Result<Vec<InsertedTaskPromptModel>, Error> {
    debug!("Inserting task_prompt models: {:?}", args);

    // Completeness guard. `insert_task_prompts.sql` re-fetches existing rows via
    // its `all_links` UNION ALL, so an ON CONFLICT (idempotent re-link) does NOT
    // reduce the row count — only the `resolved` CTE's JOIN chain can
    // (a task_id/prompt_id that fails to resolve to a live prompt path). A short
    // count therefore means a task→prompt link was silently dropped; surface it
    // (rolls back the tx) rather than ship a task pointing at a missing prompt.
    let expected = args.task_ids.len();
    let task_prompts = sqlx::query_file_as!(
        InsertedTaskPromptModel,
        "assets/.sql/tasks/task_prompts/insert_task_prompts.sql",
        &args.task_ids,
        &args.prompt_ids
    )
    .fetch_all(&mut **tx)
    .await?;

    if task_prompts.len() != expected {
        return Err(Error::Protocol(format!(
            "task_prompts insert returned {} rows but {expected} (task_id, prompt_id) \
             pairs were submitted; a link failed to resolve — aborting to avoid a task \
             with a missing prompt",
            task_prompts.len()
        )));
    }

    debug!("Inserted task_prompt models: {:?}", task_prompts);

    Ok(task_prompts)
}

pub async fn select_task_prompts_by_object_ids(
    pool: &DbPool,
    object_ids: Vec<Uuid>,
) -> Result<Vec<TaskPrompt>, DataError> {
    if object_ids.is_empty() {
        return Ok(Vec::new());
    }

    let models = sqlx::query_file_as!(
        InsertedTaskPromptModel,
        "assets/.sql/tasks/task_prompts/select_task_prompts_by_object_ids.sql",
        &object_ids,
    )
    .fetch_all(pool.get_pool())
    .await?;

    Ok(models
        .into_iter()
        .map(|m| {
            let model: TaskPromptModel = m.into();
            model.into()
        })
        .collect())
}

pub async fn select_task_prompts_by_task_object_ids(
    pool: &DbPool,
    task_object_ids: Vec<Uuid>,
) -> Result<Vec<TaskPrompt>, DataError> {
    if task_object_ids.is_empty() {
        return Ok(Vec::new());
    }

    let models = sqlx::query_file_as!(
        InsertedTaskPromptModel,
        "assets/.sql/tasks/task_prompts/select_task_prompts_by_task_object_ids.sql",
        &task_object_ids,
    )
    .fetch_all(pool.get_pool())
    .await?;

    Ok(models
        .into_iter()
        .map(|m| {
            let model: TaskPromptModel = m.into();
            model.into()
        })
        .collect())
}

/// Select a task_prompt by task ID and prompt path name
pub async fn select_task_prompt_by_task_id_and_prompt_name(
    pool: &DbPool,
    task_id: Uuid,
    prompt_name: &str,
) -> Result<Option<TaskPrompt>, DataError> {
    let model = sqlx::query_file_as!(
        InsertedTaskPromptModel,
        "assets/.sql/tasks/task_prompts/select_task_prompt_by_task_id_and_prompt_name.sql",
        task_id,
        prompt_name
    )
    .fetch_optional(pool.get_pool())
    .await?;

    Ok(model.map(|m| {
        let model: TaskPromptModel = m.into();
        model.into()
    }))
}

/// Select all task_prompts for a step_task by its ID
/// Used by Nu commands to build prompt_mappings (name -> id)
pub async fn select_task_prompts_by_step_task_id(
    pool: &DbPool,
    step_task_id: Uuid,
) -> Result<Vec<TaskPrompt>, DataError> {
    let models = sqlx::query_file_as!(
        InsertedTaskPromptModel,
        "assets/.sql/tasks/task_prompts/select_task_prompts_by_step_task_id.sql",
        step_task_id
    )
    .fetch_all(pool.get_pool())
    .await?;

    Ok(models
        .into_iter()
        .map(|m| {
            let model: TaskPromptModel = m.into();
            model.into()
        })
        .collect())
}
