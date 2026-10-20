use super::{json::Decoder, state::TextState};
use crate::{Config, Provider, Shared};
use crossbeam_queue::ArrayQueue;
use serde_json::{Value, json};
use std::sync::{
    Mutex,
    atomic::{AtomicBool, AtomicU64},
};
use subveil_core::subtitle::{SubtitleState, TranslationState};

pub(super) fn shared() -> Shared {
    Shared {
        stop: AtomicBool::new(false),
        events: ArrayQueue::new(64),
        translations: ArrayQueue::new(64),
        snapshot: Mutex::new(Default::default()),
        dropped: AtomicU64::new(0),
    }
}
pub(super) fn collect(shared: &Shared) -> SubtitleState {
    let mut state = SubtitleState::new(7, 32, 8_000);
    while let Some(event) = shared.events.pop() {
        state.apply(event, 100);
    }
    while let Some(event) = shared.translations.pop() {
        state.apply_provider_translation(event.key, event.text, event.is_final, 101);
    }
    state
}
fn fixture(provider: Provider, events: &[Value]) -> anyhow::Result<SubtitleState> {
    let config = Config {
        provider,
        ..Default::default()
    };
    let shared = shared();
    let mut next = 1;
    let mut state = TextState::new(&config, 7, &mut next);
    state.audio_ms = 1_000;
    let mut decoder = Decoder::new(provider);
    for event in events {
        decoder.parse(&event.to_string(), &mut state, &shared)?;
    }
    Ok(collect(&shared))
}
pub(super) fn bilingual(state: &SubtitleState, source: &str, target: &str, final_: bool) {
    let row = state.current().unwrap_or_else(|| panic!("missing source"));
    assert_eq!(
        (row.transcript.text.as_str(), row.is_final),
        (source, final_)
    );
    assert!(
        matches!(&row.translation,TranslationState::Complete(t)|TranslationState::Streaming(t) if t==target)
    );
}
#[test]
fn alibaba_associates_translation_item_to_explicit_source_and_deduplicates_events()
-> anyhow::Result<()> {
    let delta = json!({"type":"conversation.item.input_audio_transcription.delta","event_id":"s1","item_id":"source","delta":"Hello"});
    let state = fixture(
        Provider::Alibaba,
        &[
            delta.clone(),
            delta,
            json!({"type":"conversation.item.created","previous_item_id":"source","item":{"id":"target","role":"assistant"}}),
            json!({"type":"response.text.delta","item_id":"target","delta":"你好"}),
            json!({"type":"response.text.done","item_id":"target","text":"你好"}),
            json!({"type":"conversation.item.input_audio_transcription.completed","item_id":"source","transcript":"Hello"}),
        ],
    )?;
    bilingual(&state, "Hello", "你好", true);
    assert_eq!(state.history().len(), 1);
    Ok(())
}
#[test]
fn gemini_continuous_text_stays_partial_and_does_not_play_audio() -> anyhow::Result<()> {
    let state = fixture(
        Provider::Gemini,
        &[
            json!({"setupComplete":{}}),
            json!({"serverContent":{"inputTranscription":{"text":"Hello"},"outputTranscription":{"text":"你"},"modelTurn":{"parts":[{"inlineData":{"data":"not played"}}]}}}),
            json!({"serverContent":{"outputTranscription":{"text":"好"},"turnComplete":true}}),
        ],
    )?;
    bilingual(&state, "Hello", "你好", false);
    Ok(())
}
#[test]
fn tencent_uses_sentence_id_and_final_flag() -> anyhow::Result<()> {
    let state = fixture(
        Provider::Tencent,
        &[
            json!({"code":0,"sentence_id":"s","result":{"source_text":"Hello","target_text":"你好","start_time":0,"end_time":900,"sentence_end":true}}),
            json!({"code":0,"sentence_id":"s","result":{"source_text":"late","target_text":"wrong","start_time":0,"end_time":900,"sentence_end":false}}),
        ],
    )?;
    bilingual(&state, "Hello", "你好", true);
    Ok(())
}
#[test]
fn baidu_final_sentence_does_not_append_next_asr_preview() -> anyhow::Result<()> {
    let state = fixture(
        Provider::Baidu,
        &[
            json!({"code":0,"data":{"status":"STA"}}),
            json!({"code":0,"data":{"status":"TRN","result":{"type":"MID","asr":"Hel","asr_trans":"你"}}}),
            json!({"code":0,"data":{"status":"TRN","result":{"type":"FIN","sentence":"Hello","sentence_trans":"你好","asr":"next"}}}),
        ],
    )?;
    bilingual(&state, "Hello", "你好", true);
    Ok(())
}
#[test]
fn openai_and_azure_keep_distinct_deltas_with_equal_alignment_time() -> anyhow::Result<()> {
    for provider in [Provider::OpenAi, Provider::AzureOpenAi] {
        let state = fixture(
            provider,
            &[
                json!({"type":"session.input_transcript.delta","event_id":"1","elapsed_ms":200,"delta":"Hel"}),
                json!({"type":"session.input_transcript.delta","event_id":"2","elapsed_ms":200,"delta":"lo"}),
                json!({"type":"session.output_transcript.delta","event_id":"3","elapsed_ms":400,"delta":"你好"}),
            ],
        )?;
        bilingual(&state, "Hello", "你好", false);
    }
    Ok(())
}
#[test]
fn xai_cumulative_revisions_and_committed_turns_do_not_cross_pair() -> anyhow::Result<()> {
    let state = fixture(
        Provider::Xai,
        &[
            json!({"type":"conversation.item.input_audio_transcription.updated","item_id":"a","transcript":"Hell"}),
            json!({"type":"conversation.item.input_audio_transcription.completed","item_id":"a","transcript":"Hello"}),
            json!({"type":"input_audio_buffer.committed","item_id":"a"}),
            json!({"type":"input_audio_buffer.speech_started","item_id":"b"}),
            json!({"type":"response.created","response":{"id":"r"}}),
            json!({"type":"response.output_text.done","response_id":"r","text":"你好"}),
        ],
    )?;
    bilingual(&state, "Hello", "你好", true);
    Ok(())
}
#[test]
fn custom_protocols_only_emit_source_text() -> anyhow::Result<()> {
    let dash = fixture(
        Provider::CustomDashScope,
        &[
            json!({"header":{"event":"result-generated"},"payload":{"output":{"sentence":{"begin_time":0,"end_time":100,"text":"Hello","sentence_end":true}}}}),
        ],
    )?;
    let open = fixture(
        Provider::CustomOpenAi,
        &[
            json!({"type":"conversation.item.input_audio_transcription.completed","item_id":"i","transcript":"Hello"}),
        ],
    )?;
    for state in [dash, open] {
        assert!(state.current().is_some_and(|r| r.is_final
            && r.transcript.text == "Hello"
            && r.translation == TranslationState::NotRequested));
    }
    Ok(())
}
#[test]
fn late_translation_with_unknown_link_is_not_assigned_to_latest_speech() -> anyhow::Result<()> {
    let state = fixture(
        Provider::Alibaba,
        &[
            json!({"type":"conversation.item.input_audio_transcription.completed","item_id":"new","transcript":"Hello"}),
            json!({"type":"response.text.done","item_id":"unknown","text":"wrong"}),
        ],
    )?;
    assert!(
        state
            .current()
            .is_some_and(|r| r.translation == TranslationState::NotRequested)
    );
    Ok(())
}
#[test]
fn service_errors_never_echo_payloads() -> anyhow::Result<()> {
    for provider in Provider::ALL
        .into_iter()
        .filter(|p| !matches!(p, Provider::Mock | Provider::Deepgram | Provider::Volcano))
    {
        let config = Config {
            provider,
            ..Default::default()
        };
        let shared = shared();
        let mut next = 1;
        let mut state = TextState::new(&config, 7, &mut next);
        let mut decoder = Decoder::new(provider);
        let error = decoder
            .parse(
                r#"{"type":"error","error":{"message":"private transcript and key"}}"#,
                &mut state,
                &shared,
            )
            .err()
            .map(|e| e.to_string())
            .unwrap_or_default();
        assert!(!error.is_empty() && !error.contains("private transcript"));
    }
    Ok(())
}
#[test]
fn translation_before_source_is_buffered_and_retired_ids_stay_retired() -> anyhow::Result<()> {
    let config = Config::default();
    let shared = shared();
    let mut next = 1;
    let mut state = TextState::new(&config, 7, &mut next);
    state.target("a", "你好", false, true, &shared)?;
    assert!(shared.translations.is_empty());
    state.source("a", "Hello", false, true, 0, 100, &shared)?;
    for i in 0..65 {
        state.source(&format!("s{i}"), "next", false, true, 100, 200, &shared)?;
    }
    state.source("a", "stale", false, true, 0, 100, &shared)?;
    let state = collect(&shared);
    assert!(!state.history().iter().any(|s| s.transcript.text == "stale"));
    Ok(())
}
#[test]
fn continuous_stream_storage_stays_bounded_on_unicode() -> anyhow::Result<()> {
    let mut events = vec![json!({"type":"session.output_transcript.delta","delta":"你好"})];
    events.extend(
        (0..100).map(|_| json!({"type":"session.input_transcript.delta","delta":"日".repeat(100)})),
    );
    let state = fixture(Provider::OpenAi, &events)?;
    assert!(
        state
            .current()
            .is_some_and(|r| r.transcript.text.len() <= 8192)
    );
    Ok(())
}
