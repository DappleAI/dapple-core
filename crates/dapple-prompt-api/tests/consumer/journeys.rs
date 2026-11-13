use fake::Fake;
use std::ops::Range;
use std::sync::{Arc, Mutex};

use crate::consumer::{Endpoint, ScenarioContext};
use dapple_data::model::journeys::Journey;
use dapple_prompt_api::schema::journeys::{
    CreateJourneySchema, CreateJourneySetSchema, GetJourneySetSchema,
};
use reqwest::Url;
use uuid::Uuid;

use crate::consumer::error::ConsumerError;
use log::info;

pub struct JourneyEndpoint {
    pub base_url: Url,
    pub context: Arc<Mutex<ScenarioContext>>,
}

impl JourneyEndpoint {
    pub fn fake_create_schema(&self) -> <JourneyEndpoint as Endpoint>::CreateSchema {
        self.fake_create_schema_with_child_count(2)
    }

    fn fake_create_schema_with_child_count(&self, count: usize) -> CreateJourneySchema {
        let guard = self.context.lock().unwrap_or_else(|p| p.into_inner());
        let playbook_universal_id = guard
            .random_playbook_from_any_batch()
            .map(|pb| pb.id)
            .expect("fake_create_schema_with_child_count: no playbooks available in any registered batch — register playbooks before creating journeys");

        // Every journey is version-pinned to a guide, and the handler now
        // VALIDATES that the guide exposes the chosen playbook (400
        // GUIDE_PLAYBOOK_MISMATCH otherwise) — there is no auto-pin. The
        // journeys lifecycle trial seeds `rev.guide_playbooks` links covering
        // the cross-product of the registered guides × the journey playbooks
        // before posting, so whichever (guide, playbook) pair we pick here is
        // guaranteed to be linked.
        let guide_id = guard
            .random_guide_from_any_batch()
            .map(|g| g.id)
            .expect("fake_create_schema_with_child_count: no guides available — register guides before creating journeys");

        // Get brief packs from context (if available)
        let brief_pack_ids: Vec<Uuid> = guard
            .brief_pack_batches
            .iter()
            .flat_map(|batch| batch.iter().map(|bp| bp.id))
            .take(count)
            .collect();

        // Get inference profiles from context (if available)
        let inference_profile_ids: Vec<Uuid> = guard
            .inference_profile_batches
            .iter()
            .flat_map(|batch| batch.iter().map(|ip| ip.id))
            .take(count)
            .collect();

        CreateJourneySchema {
            name: Self::fake_name(),
            playbook_id: playbook_universal_id,
            brief_pack_ids,
            inference_profile_ids,
            predecessor_journey_id: None,
            fork_stage_id: None,
            guide_id,
        }
    }

    pub fn fake_create_set_schema(
        &self,
        size: Range<usize>,
    ) -> <JourneyEndpoint as Endpoint>::CreateSetSchema {
        let fakes = (size).fake::<usize>();

        CreateJourneySetSchema {
            objs: (0..fakes)
                .map(|i| {
                    let count = if i == 0 { 1 } else { 3 };
                    self.fake_create_schema_with_child_count(count)
                })
                .collect(),
        }
    }
}

impl Endpoint for JourneyEndpoint {
    type Object = Journey;
    type GetSchema = GetJourneySetSchema;
    type GetSetSchema = GetJourneySetSchema;
    type CreateSchema = CreateJourneySchema;
    type CreateSetSchema = CreateJourneySetSchema;

    fn path(&self) -> &str {
        "journeys"
    }

    fn base_url(&self) -> Url {
        self.base_url.clone()
    }

    async fn test_fetched_object_id(
        &self,
        object_id: Uuid,
        obj: &Self::Object,
    ) -> Result<(), ConsumerError> {
        assert_eq!(obj.id, object_id, "fetched object id");
        Ok(())
    }

    async fn test_returned_set(&self, objs: &Vec<Self::Object>) -> Result<(), ConsumerError> {
        for obj in objs {
            let fetched = self.get_object(obj.id).await?;
            assert_eq!(obj, &fetched, "fetched object does not match original");
        }
        Ok(())
    }

    async fn test_returned_post_set(
        &self,
        schema: &Self::CreateSetSchema,
        objs: &Vec<Self::Object>,
    ) -> Result<(), ConsumerError> {
        assert_eq!(schema.objs.len(), objs.len());

        for s in schema.objs.iter() {
            let o = objs
                .iter()
                .find(|o| o.name == s.name)
                .expect("response missing journey with matching name");
            // verify playbook id matches
            assert_eq!(s.playbook_id, o.playbook_id, "playbook_id");

            // Verify brief packs match
            let expected_bp_ids: Vec<Uuid> = s.brief_pack_ids.clone();
            let actual_bp_ids: Vec<Uuid> = o.brief_packs.iter().map(|bp| bp.id).collect();
            assert_eq!(expected_bp_ids, actual_bp_ids, "brief_pack_ids mismatch");

            // Verify inference profiles match
            let expected_ip_ids: Vec<Uuid> = s.inference_profile_ids.clone();
            let actual_ip_ids: Vec<Uuid> = o.inference_profiles.iter().map(|ip| ip.id).collect();
            assert_eq!(
                expected_ip_ids, actual_ip_ids,
                "inference_profile_ids mismatch"
            );
        }

        info!("requested count: {}", schema.objs.len());
        info!(
            "POSTed set of {} journeys successfully verified",
            objs.len()
        );
        info!("response results size: {}", objs.len());
        info!(
            "Created journeys IDs: {:?}",
            objs.iter().map(|o| o.id).collect::<Vec<Uuid>>()
        );
        info!(
            "Attached brief packs: {:?}",
            objs.iter()
                .map(|o| o.brief_packs.len())
                .collect::<Vec<usize>>()
        );
        info!(
            "Attached inference profiles: {:?}",
            objs.iter()
                .map(|o| o.inference_profiles.len())
                .collect::<Vec<usize>>()
        );

        Ok(())
    }

    fn fake_create_schema(&self) -> Self::CreateSchema {
        self.fake_create_schema()
    }

    fn fake_create_set_schema(&self, size: Range<usize>) -> Self::CreateSetSchema {
        self.fake_create_set_schema(size)
    }
}
