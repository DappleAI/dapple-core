//! Custom Nushell commands for dapple-prompt-flow
//!
//! This module provides prompt-related commands that integrate with the conduit service.

pub mod animate;
pub mod asset;
pub mod asset_status;
pub mod asset_text;
pub mod audio;
pub mod audio_analyze;
pub mod av;
pub mod av_compose;
pub mod av_mix;
pub mod av_render;
pub mod av_snapshot;
pub mod binding;
pub mod composite;
pub mod dataset;
pub mod dataset_open;
pub mod dataset_schema;
pub mod db_access;
pub mod document;
pub mod document_render;
pub mod edgar;
pub mod edgar_adv;
pub mod edgar_advised;
pub mod edgar_bdc;
pub mod edgar_book;
pub mod edgar_common;
pub mod edgar_company_cik;
pub mod edgar_exhibits;
pub mod edgar_facts;
pub mod edgar_filings;
pub mod edgar_fund;
pub mod edgar_fund_sections;
pub mod edgar_holders;
pub mod edgar_insiders;
pub mod edgar_manager;
pub mod edgar_ncen;
pub mod edgar_nmfp;
pub mod edgar_nmfp_universe;
pub mod edgar_nport;
pub mod edgar_portfolio;
pub mod edgar_resolve;
pub mod edgar_search;
pub mod edgar_security;
pub mod edgar_segments;
pub mod fluent;
pub mod fluent_emit;
pub mod fmt;
pub mod fmt_round;
pub mod fmt_scale;
pub mod fmt_usd;
pub mod guide;
pub mod guide_search;
pub mod imager;
pub mod imager_compose;
pub mod imagine;
pub mod json_schema;
pub mod madlib;
pub mod md;
pub mod md_admonish;
pub mod md_table;
pub mod media_gateway;
pub mod prompt;
pub mod prompt_reset;
pub mod prompt_retry;
pub mod prompt_send;
pub mod summarize;
pub mod svg;
pub mod svg_shapes;
pub mod synth;
pub mod text;
pub mod text_window;
pub mod twitch;
pub mod twitch_pull;
pub mod vod;
pub mod vod_analyze;
pub mod vod_assemble;
pub mod vod_context;
pub mod vod_cut;
pub mod vod_detect;
pub mod vod_job;
pub mod vod_transcribe;

use nu_protocol::engine::{EngineState, StateWorkingSet};

/// Register all dapple custom commands with the engine state
pub fn register_dapple_commands(engine_state: &mut EngineState) {
    let mut working_set = StateWorkingSet::new(engine_state);

    // Register the prompt parent command
    working_set.add_decl(Box::new(prompt::PromptCommand));

    // Register prompt subcommands
    working_set.add_decl(Box::new(prompt_send::PromptSend));
    working_set.add_decl(Box::new(prompt_retry::PromptRetry));
    working_set.add_decl(Box::new(prompt_reset::PromptReset));

    // Register summarize command
    working_set.add_decl(Box::new(summarize::SummarizeCommand));

    // Register madlib command
    working_set.add_decl(Box::new(madlib::MadlibCommand));

    // Register json-schema command
    working_set.add_decl(Box::new(json_schema::JsonSchemaCommand));

    // Register synth command
    working_set.add_decl(Box::new(synth::SynthCommand));

    // Register image commands: `imagine` (the only generative call — produces
    // net-new pieces) + `composite` (low-level, caller-positioned assembly) +
    // `imager compose` (strategy-driven, content-aware assembly).
    working_set.add_decl(Box::new(imagine::ImagineCommand));
    working_set.add_decl(Box::new(composite::CompositeCommand));
    working_set.add_decl(Box::new(imager::ImagerCommand));
    working_set.add_decl(Box::new(imager_compose::ImagerComposeCommand));

    // Video generation: `animate` (the video analog of `imagine` — the only
    // generative video call; deterministic assembly lives in `av compose`).
    working_set.add_decl(Box::new(animate::AnimateCommand));

    // A/V timeline assembly: `av compose` (audio+video analog of `imager compose`;
    // `--strategy document` submits a guide-compiled composition document), plus
    // `av snapshot` (clip final frames) and `av render` (composition manifest →
    // final mp4 via the media gateway).
    working_set.add_decl(Box::new(av::AvCommand));
    working_set.add_decl(Box::new(av_compose::AvComposeCommand));
    working_set.add_decl(Box::new(av_mix::AvMixCommand));
    working_set.add_decl(Box::new(av_render::AvRenderCommand));
    working_set.add_decl(Box::new(av_snapshot::AvSnapshotCommand));

    // Audio analysis: `audio analyze` (multitrack WAV stems → song map) via the
    // conduit-oximedia media service.
    working_set.add_decl(Box::new(audio::AudioCommand));
    working_set.add_decl(Box::new(audio_analyze::AudioAnalyzeCommand));

    // SVG mechanics: `svg shapes` extracts basic shape geometry so guide tasks
    // can compile their own composition documents — shape MEANING stays guide-side.
    working_set.add_decl(Box::new(svg::SvgCommand));
    working_set.add_decl(Box::new(svg_shapes::SvgShapesCommand));

    // Register fluent parent command + subcommands
    working_set.add_decl(Box::new(fluent::FluentCommand));
    working_set.add_decl(Box::new(fluent_emit::FluentEmitGuide));

    // Register guide parent command and subcommands
    working_set.add_decl(Box::new(guide::GuideCommand));
    working_set.add_decl(Box::new(guide_search::GuideSearch));

    // Register asset parent command and subcommands
    working_set.add_decl(Box::new(asset::Asset));
    working_set.add_decl(Box::new(asset_text::AssetText));
    working_set.add_decl(Box::new(asset_status::AssetStatus));

    // Dataset (spreadsheet / tabular) commands
    working_set.add_decl(Box::new(dataset::Dataset));
    working_set.add_decl(Box::new(dataset_schema::DatasetSchema));
    working_set.add_decl(Box::new(dataset_open::DatasetOpen));

    // Value-slot binding commands (envelope-delivery handle resolution)
    working_set.add_decl(Box::new(binding::Binding));
    working_set.add_decl(Box::new(binding::BindingOpen));

    // Media gateway: Twitch VOD → learning-highlight clips (opt-in service,
    // reached at DAPPLE_MEDIA_GATEWAY_URL).
    working_set.add_decl(Box::new(twitch::TwitchCommand));
    working_set.add_decl(Box::new(twitch_pull::TwitchPullCommand));
    working_set.add_decl(Box::new(vod::VodCommand));
    working_set.add_decl(Box::new(vod_analyze::VodAnalyzeCommand));
    working_set.add_decl(Box::new(vod_transcribe::VodTranscribeCommand));
    working_set.add_decl(Box::new(vod_detect::VodDetectCommand));
    working_set.add_decl(Box::new(vod_context::VodContextCommand));
    working_set.add_decl(Box::new(vod_assemble::VodAssembleCommand));
    working_set.add_decl(Box::new(vod_cut::VodCutCommand));
    working_set.add_decl(Box::new(vod_job::VodJobCommand));

    // SEC EDGAR: 10-K filings sectioned by Item + XBRL facts (opt-in at call
    // time via DAPPLE_EDGAR_USER_AGENT — the SEC-mandated declared User-Agent).
    working_set.add_decl(Box::new(edgar::EdgarCommand));
    working_set.add_decl(Box::new(edgar_filings::EdgarFilings));
    working_set.add_decl(Box::new(edgar_facts::EdgarFacts));
    working_set.add_decl(Box::new(edgar_exhibits::EdgarExhibits));
    working_set.add_decl(Box::new(edgar_holders::EdgarHolders));
    working_set.add_decl(Box::new(edgar_insiders::EdgarInsiders));
    working_set.add_decl(Box::new(edgar_segments::EdgarSegments));
    working_set.add_decl(Box::new(edgar_manager::EdgarManager));
    working_set.add_decl(Box::new(edgar_security::EdgarSecurity));
    working_set.add_decl(Box::new(edgar_fund::EdgarFund));
    working_set.add_decl(Box::new(edgar_nport::EdgarNport));
    working_set.add_decl(Box::new(edgar_ncen::EdgarNcen));
    working_set.add_decl(Box::new(edgar_nmfp::EdgarNmfp));
    working_set.add_decl(Box::new(edgar_nmfp_universe::EdgarNmfpUniverse));
    working_set.add_decl(Box::new(edgar_fund_sections::EdgarFundSections));
    working_set.add_decl(Box::new(edgar_resolve::EdgarResolve));
    working_set.add_decl(Box::new(edgar_company_cik::EdgarCompanyCik));
    working_set.add_decl(Box::new(edgar_search::EdgarSearch));
    working_set.add_decl(Box::new(edgar_adv::EdgarAdv));
    working_set.add_decl(Box::new(edgar_advised::EdgarAdvised));
    working_set.add_decl(Box::new(edgar_portfolio::EdgarPortfolio));
    working_set.add_decl(Box::new(edgar_bdc::EdgarBdc));
    working_set.add_decl(Box::new(edgar_book::EdgarBook));

    // Document rendering: `document render` turns a task's final markdown into
    // a PDF stored as a `document` asset (the report-deliverable path).
    working_set.add_decl(Box::new(document::DocumentCommand));
    working_set.add_decl(Box::new(document_render::DocumentRenderCommand));

    // Shared report furniture. Guide scripts cannot import each other, so these
    // lived as copy-pasted closures and drifted: one `md_table` arrived without
    // its cell scrubber, and one `scale_num` dropped the sign it stripped.
    // `md` is markdown emission; `fmt` is number display and is deliberately
    // separate, because guides that render no document format numbers too.
    working_set.add_decl(Box::new(md::MdCommand));
    working_set.add_decl(Box::new(md_table::MdTable));
    working_set.add_decl(Box::new(md_admonish::MdAdmonish));
    working_set.add_decl(Box::new(fmt::FmtCommand));
    working_set.add_decl(Box::new(fmt_scale::FmtScale));
    working_set.add_decl(Box::new(fmt_usd::FmtUsd));
    working_set.add_decl(Box::new(fmt_round::FmtRound));

    // Long-text sampling. `str substring 0..N` is a bet that the first N chars
    // matter most, and on a shareholder letter that bet captures the good news
    // and cuts the carve-out every time.
    working_set.add_decl(Box::new(text::TextCommand));
    working_set.add_decl(Box::new(text_window::TextWindow));

    let _ = engine_state.merge_delta(working_set.render());
}
