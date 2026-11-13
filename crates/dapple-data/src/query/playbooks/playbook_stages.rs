use crate::error::DataError;
use crate::model::playbooks::playbook_stages::InsertedPlaybookStageModel;
use sqlx::{Postgres, Transaction, query_file_as};

pub async fn insert_playbook_stages_tx(
    tx: &mut Transaction<'static, Postgres>,
    playbook_ids: Vec<sqlx::types::Uuid>,
    stage_ids: Vec<sqlx::types::Uuid>,
) -> Result<Vec<InsertedPlaybookStageModel>, DataError> {
    let inserted = query_file_as!(
        InsertedPlaybookStageModel,
        "assets/.sql/playbooks/playbook_stages/insert_playbook_stages.sql",
        &playbook_ids,
        &stage_ids
    )
    .fetch_all(&mut **tx)
    .await?;

    Ok(inserted)
}
