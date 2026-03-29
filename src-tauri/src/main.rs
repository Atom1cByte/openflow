#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::env;
use std::fs;
use std::io::Cursor;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::Context;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use enigo::{Enigo, Key, KeyboardControllable};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, GlobalShortcutManager, Manager, PhysicalPosition, State, WindowEvent};

#[cfg(target_os = "windows")]
use winreg::{enums::HKEY_CURRENT_USER, RegKey};

#[derive(Clone, Serialize, Deserialize, Default)]
#[serde(default)]
struct Settings {
  groq_api_key: String,
  hotkey_dictate: String,
  hotkey_command: String,
  hotkey_hands_free: String,
  whisper_model: String,
  chat_model: String,
  enable_llm_enhancement: bool,
  launch_on_startup: bool,
  bar_hidden: bool,
  bar_x: Option<f64>,
  bar_y: Option<f64>,
}

impl Settings {
  fn fresh() -> Self {
    Self {
      groq_api_key: String::new(),
      hotkey_dictate: "Ctrl+Shift+D".into(),
      hotkey_command: "Ctrl+Shift+E".into(),
      hotkey_hands_free: "Ctrl+Shift+F".into(),
      whisper_model: "whisper-large-v3-turbo".into(),
      chat_model: "llama-3.1-8b-instant".into(),
      enable_llm_enhancement: true,
      launch_on_startup: false,
      bar_hidden: false,
      bar_x: None,
      bar_y: None,
    }
  }
}

#[derive(Clone)]
struct AppState {
  settings: Arc<Mutex<Settings>>,
  recording: Arc<AtomicBool>,
  buffer: Arc<Mutex<Vec<i16>>>,
  sample_rate: Arc<Mutex<u32>>,
  stop_tx: Arc<Mutex<Option<mpsc::Sender<()>>>>,
  active_hotkey: Arc<Mutex<String>>,
  hotkey_registered: Arc<AtomicBool>,
  last_hotkey_error: Arc<Mutex<String>>,
}

#[derive(Clone, Serialize)]
struct RuntimeStatus {
  active_hotkey: String,
  hotkey_registered: bool,
  last_hotkey_error: String,
  bar_hidden: bool,
}

fn sanitize_settings(settings: &mut Settings) {
  if settings.hotkey_dictate.trim().is_empty() || settings.hotkey_dictate == "Ctrl+Win" {
    settings.hotkey_dictate = "Ctrl+Shift+D".into();
  }
  if settings.hotkey_command.trim().is_empty() || settings.hotkey_command == "Ctrl+Win+Alt" {
    settings.hotkey_command = "Ctrl+Shift+E".into();
  }
  if settings.hotkey_hands_free.trim().is_empty() || settings.hotkey_hands_free == "Ctrl+Win+Shift" {
    settings.hotkey_hands_free = "Ctrl+Shift+F".into();
  }
  if settings.whisper_model.trim().is_empty() {
    settings.whisper_model = "whisper-large-v3-turbo".into();
  }
  if settings.chat_model.trim().is_empty() {
    settings.chat_model = "llama-3.1-8b-instant".into();
  }
}

fn normalize_hotkey(shortcut: &str, fallback_key: &str) -> String {
  let mut parts = Vec::new();
  let mut saw_ctrl = false;
  let mut saw_alt = false;
  let mut saw_shift = false;
  let mut saw_primary = false;

  for token in shortcut.split('+').map(|part| part.trim().to_lowercase()) {
    match token.as_str() {
      "ctrl" | "control" => {
        if !saw_ctrl {
          parts.push("CmdOrControl".to_string());
          saw_ctrl = true;
        }
      }
      "alt" => {
        if !saw_alt {
          parts.push("Alt".to_string());
          saw_alt = true;
        }
      }
      "shift" => {
        if !saw_shift {
          parts.push("Shift".to_string());
          saw_shift = true;
        }
      }
      "win" | "super" | "meta" | "cmd" => {}
      "space" => {
        parts.push("Space".to_string());
        saw_primary = true;
      }
      key if key.starts_with('f') && key[1..].chars().all(|ch| ch.is_ascii_digit()) => {
        parts.push(key.to_uppercase());
        saw_primary = true;
      }
      key if key.len() == 1 => {
        parts.push(key.to_uppercase());
        saw_primary = true;
      }
      _ => {}
    }
  }

  if !saw_primary {
    parts.push(fallback_key.to_uppercase());
  }

  parts.join("+")
}

fn settings_path(app: &AppHandle) -> PathBuf {
  let mut path = app
    .path_resolver()
    .app_config_dir()
    .unwrap_or_else(|| app.path_resolver().app_data_dir().unwrap());
  path.push("settings.json");
  path
}

fn load_settings(app: &AppHandle) -> Settings {
  let path = settings_path(app);
  if let Ok(raw) = fs::read_to_string(path) {
    if let Ok(mut settings) = serde_json::from_str::<Settings>(&raw) {
      sanitize_settings(&mut settings);
      return settings;
    }
  }
  Settings::fresh()
}

fn save_settings_file(app: &AppHandle, settings: &Settings) -> anyhow::Result<()> {
  let path = settings_path(app);
  if let Some(parent) = path.parent() {
    fs::create_dir_all(parent)?;
  }
  fs::write(path, serde_json::to_string_pretty(settings)?)?;
  Ok(())
}

fn current_settings(state: &AppState) -> Settings {
  state.settings.lock().unwrap().clone()
}

fn runtime_status(state: &AppState) -> RuntimeStatus {
  RuntimeStatus {
    active_hotkey: state.active_hotkey.lock().unwrap().clone(),
    hotkey_registered: state.hotkey_registered.load(Ordering::SeqCst),
    last_hotkey_error: state.last_hotkey_error.lock().unwrap().clone(),
    bar_hidden: state.settings.lock().unwrap().bar_hidden,
  }
}

#[cfg(target_os = "windows")]
fn apply_startup_setting(settings: &Settings) -> Result<(), String> {
  let hkcu = RegKey::predef(HKEY_CURRENT_USER);
  let (key, _) = hkcu
    .create_subkey("Software\\Microsoft\\Windows\\CurrentVersion\\Run")
    .map_err(|error| error.to_string())?;

  if settings.launch_on_startup {
    let executable = env::current_exe().map_err(|error| error.to_string())?;
    let value = format!("\"{}\" --startup", executable.display());
    key.set_value("OpenFlow", &value).map_err(|error| error.to_string())?;
  } else {
    let _ = key.delete_value("OpenFlow");
  }

  Ok(())
}

#[cfg(not(target_os = "windows"))]
fn apply_startup_setting(_settings: &Settings) -> Result<(), String> {
  Ok(())
}

fn update_bar_visibility(app: &AppHandle, hidden: bool) -> Result<(), String> {
  let Some(bar) = app.get_window("bar") else {
    return Ok(());
  };

  if hidden {
    bar.hide().map_err(|error| error.to_string())
  } else {
    bar.show().map_err(|error| error.to_string())?;
    bar.set_focus().map_err(|error| error.to_string())
  }
}

fn store_bar_position(app: &AppHandle, x: f64, y: f64) {
  let state = app.state::<AppState>();
  let updated = {
    let mut settings = state.settings.lock().unwrap();
    settings.bar_x = Some(x);
    settings.bar_y = Some(y);
    settings.clone()
  };
  let _ = save_settings_file(app, &updated);
}

fn default_bar_position(bar: &tauri::Window) -> Result<PhysicalPosition<f64>, String> {
  if let Some(monitor) = bar.current_monitor().map_err(|error| error.to_string())? {
    let size = monitor.size();
    return Ok(PhysicalPosition::new(
      (size.width as f64 - 180.0) / 2.0,
      size.height as f64 - 92.0,
    ));
  }

  Ok(PhysicalPosition::new(960.0, 980.0))
}

fn apply_bar_position(bar: &tauri::Window, settings: &Settings) -> Result<(), String> {
  let position = match (settings.bar_x, settings.bar_y) {
    (Some(x), Some(y)) => PhysicalPosition::new(x, y),
    _ => default_bar_position(bar)?,
  };
  bar.set_position(position).map_err(|error| error.to_string())
}

fn emit_bar(app: &AppHandle, status: &str) {
  if let Some(bar) = app.get_window("bar") {
    let _ = bar.emit("bar_status", serde_json::json!({ "status": status }));
  }
}

fn register_primary_hotkey(app: &AppHandle) -> Result<(), String> {
  let raw_shortcut = app.state::<AppState>().settings.lock().unwrap().hotkey_dictate.clone();
  let normalized = normalize_hotkey(&raw_shortcut, "D");

  let candidates = vec![normalized, "CmdOrControl+Shift+D".into(), "F8".into()];
  let mut manager = app.global_shortcut_manager();
  let _ = manager.unregister_all();

  {
    let state = app.state::<AppState>();
    state.hotkey_registered.store(false, Ordering::SeqCst);
    *state.last_hotkey_error.lock().unwrap() = String::new();
  }

  let mut last_error = String::from("Unable to register any global shortcut");
  for candidate in candidates {
    let app_for_handler = app.clone();
    match manager.register(&candidate, move || {
      let state = app_for_handler.state::<AppState>();
      let _ = toggle_recording(app_for_handler.clone(), state);
    }) {
      Ok(_) => {
        let state = app.state::<AppState>();
        *state.active_hotkey.lock().unwrap() = candidate;
        state.hotkey_registered.store(true, Ordering::SeqCst);
        *state.last_hotkey_error.lock().unwrap() = String::new();
        return Ok(());
      }
      Err(error) => last_error = error.to_string(),
    }
  }

  let state = app.state::<AppState>();
  *state.active_hotkey.lock().unwrap() = String::new();
  state.hotkey_registered.store(false, Ordering::SeqCst);
  *state.last_hotkey_error.lock().unwrap() = last_error.clone();
  Err(last_error)
}

#[tauri::command]
fn get_settings(state: State<AppState>) -> Settings {
  current_settings(&state)
}

#[tauri::command]
fn get_runtime_status(state: State<AppState>) -> RuntimeStatus {
  runtime_status(&state)
}

#[tauri::command]
fn save_settings(app: AppHandle, state: State<AppState>, settings: Settings) -> Result<(), String> {
  let mut merged = settings;
  sanitize_settings(&mut merged);
  *state.settings.lock().unwrap() = merged.clone();
  save_settings_file(&app, &merged).map_err(|error| error.to_string())?;
  apply_startup_setting(&merged)?;
  update_bar_visibility(&app, merged.bar_hidden)?;
  let _ = register_primary_hotkey(&app);
  Ok(())
}

#[tauri::command]
fn set_bar_hidden(app: AppHandle, state: State<AppState>, hidden: bool) -> Result<RuntimeStatus, String> {
  let updated = {
    let mut settings = state.settings.lock().unwrap();
    settings.bar_hidden = hidden;
    settings.clone()
  };
  save_settings_file(&app, &updated).map_err(|error| error.to_string())?;
  update_bar_visibility(&app, hidden)?;
  Ok(runtime_status(&state))
}

#[tauri::command]
fn show_hub(app: AppHandle) -> Result<(), String> {
  let window = app.get_window("main").context("Main window missing").map_err(|error| error.to_string())?;
  window.show().map_err(|error| error.to_string())?;
  window.unminimize().map_err(|error| error.to_string())?;
  window.set_focus().map_err(|error| error.to_string())
}

#[tauri::command]
fn hide_bar_and_show_hub(app: AppHandle, state: State<AppState>) -> Result<(), String> {
  {
    let mut settings = state.settings.lock().unwrap();
    settings.bar_hidden = true;
    let snapshot = settings.clone();
    save_settings_file(&app, &snapshot).map_err(|error| error.to_string())?;
  }
  update_bar_visibility(&app, true)?;
  show_hub(app)
}

#[tauri::command]
fn start_bar_drag(app: AppHandle) -> Result<(), String> {
  let window = app
    .get_window("bar")
    .context("Bar window missing")
    .map_err(|error| error.to_string())?;
  window.start_dragging().map_err(|error| error.to_string())
}

#[tauri::command]
fn toggle_recording(app: AppHandle, state: State<AppState>) -> Result<(), String> {
  if state.recording.load(Ordering::SeqCst) {
    stop_recording(app, state)
  } else {
    start_recording(app, state)
  }
}

fn start_recording(app: AppHandle, state: State<AppState>) -> Result<(), String> {
  if state.recording.swap(true, Ordering::SeqCst) {
    return Ok(());
  }

  state.buffer.lock().unwrap().clear();
  emit_bar(&app, "listening");

  let (tx, rx) = mpsc::channel::<()>();
  *state.stop_tx.lock().unwrap() = Some(tx);

  let buffer = state.buffer.clone();
  let sample_rate = state.sample_rate.clone();
  let recording_flag = state.recording.clone();
  let recording_err = state.recording.clone();

  let buffer_i16 = buffer.clone();
  let recording_i16 = recording_flag.clone();
  let buffer_u16 = buffer.clone();
  let recording_u16 = recording_flag.clone();
  let buffer_f32 = buffer;
  let recording_f32 = recording_flag.clone();

  std::thread::spawn(move || {
    let host = cpal::default_host();
    let Some(device) = host.default_input_device() else {
      recording_err.store(false, Ordering::SeqCst);
      return;
    };

    let Ok(config) = device.default_input_config() else {
      recording_err.store(false, Ordering::SeqCst);
      return;
    };

    *sample_rate.lock().unwrap() = config.sample_rate().0;
    let err_fn = |_error| {};

    let stream = match config.sample_format() {
      cpal::SampleFormat::I16 => device.build_input_stream(
        &config.clone().into(),
        move |data: &[i16], _| {
          if recording_i16.load(Ordering::SeqCst) {
            buffer_i16.lock().unwrap().extend_from_slice(data);
          }
        },
        err_fn,
        None,
      ),
      cpal::SampleFormat::U16 => device.build_input_stream(
        &config.clone().into(),
        move |data: &[u16], _| {
          if recording_u16.load(Ordering::SeqCst) {
            let mut output = buffer_u16.lock().unwrap();
            output.extend(data.iter().map(|sample| ((*sample as i32) - 32_768) as i16));
          }
        },
        err_fn,
        None,
      ),
      cpal::SampleFormat::F32 => device.build_input_stream(
        &config.into(),
        move |data: &[f32], _| {
          if recording_f32.load(Ordering::SeqCst) {
            let mut output = buffer_f32.lock().unwrap();
            output.extend(data.iter().map(|sample| (sample * i16::MAX as f32) as i16));
          }
        },
        err_fn,
        None,
      ),
      _ => {
        recording_err.store(false, Ordering::SeqCst);
        return;
      }
    };

    let Ok(stream) = stream else {
      recording_err.store(false, Ordering::SeqCst);
      return;
    };

    if stream.play().is_err() {
      recording_err.store(false, Ordering::SeqCst);
      return;
    }

    while recording_flag.load(Ordering::SeqCst) {
      if rx.try_recv().is_ok() {
        break;
      }
      std::thread::sleep(Duration::from_millis(40));
    }
  });

  Ok(())
}

fn stop_recording(app: AppHandle, state: State<AppState>) -> Result<(), String> {
  state.recording.store(false, Ordering::SeqCst);
  emit_bar(&app, "transcribing");

  if let Some(tx) = state.stop_tx.lock().unwrap().take() {
    let _ = tx.send(());
  }

  let audio = state.buffer.lock().unwrap().clone();
  let sample_rate = *state.sample_rate.lock().unwrap();
  let settings = current_settings(&state);

  tauri::async_runtime::spawn(async move {
    if settings.groq_api_key.trim().is_empty() || audio.is_empty() {
      emit_bar(&app, "idle");
      return;
    }

    let wav = match build_wav(audio, sample_rate) {
      Ok(data) => data,
      Err(_) => {
        emit_bar(&app, "idle");
        return;
      }
    };

    if let Ok(text) = groq_transcribe(&settings, wav).await {
      if !text.trim().is_empty() {
        let final_text = if settings.enable_llm_enhancement {
          groq_enhance_text(&settings, &text).await.unwrap_or(text)
        } else {
          text
        };
        let _ = paste_text(final_text);
      }
    }

    emit_bar(&app, "idle");
  });

  Ok(())
}

fn build_wav(samples: Vec<i16>, sample_rate: u32) -> anyhow::Result<Vec<u8>> {
  let mut out = Cursor::new(Vec::new());
  {
    let mut writer = hound::WavWriter::new(
      &mut out,
      hound::WavSpec {
        channels: 1,
        sample_rate,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
      },
    )?;
    for sample in samples {
      writer.write_sample(sample)?;
    }
    writer.finalize()?;
  }
  Ok(out.into_inner())
}

async fn groq_transcribe(settings: &Settings, wav: Vec<u8>) -> anyhow::Result<String> {
  let client = reqwest::Client::new();
  let audio = reqwest::multipart::Part::bytes(wav)
    .file_name("audio.wav")
    .mime_str("audio/wav")?;
  let form = reqwest::multipart::Form::new()
    .part("file", audio)
    .text("model", settings.whisper_model.clone());

  let response = client
    .post("https://api.groq.com/openai/v1/audio/transcriptions")
    .bearer_auth(settings.groq_api_key.trim())
    .multipart(form)
    .send()
    .await?;

  let payload: serde_json::Value = response.json().await?;
  Ok(payload.get("text").and_then(|value| value.as_str()).unwrap_or("").to_string())
}

async fn groq_enhance_text(settings: &Settings, text: &str) -> anyhow::Result<String> {
  let client = reqwest::Client::new();
  let body = serde_json::json!({
    "model": settings.chat_model,
    "temperature": 0.15,
    "messages": [
      {
        "role": "system",
        "content": "You clean up dictated text before it is pasted into another app. Fix punctuation, capitalization, and grammar. Remove filler words and backtracks when they do not add meaning. Keep the original intent and tone. Return only the final rewritten text."
      },
      {
        "role": "user",
        "content": text
      }
    ]
  });

  let response = client
    .post("https://api.groq.com/openai/v1/chat/completions")
    .bearer_auth(settings.groq_api_key.trim())
    .json(&body)
    .send()
    .await?;

  let payload: serde_json::Value = response.json().await?;
  Ok(
    payload["choices"][0]["message"]["content"]
      .as_str()
      .unwrap_or(text)
      .trim()
      .to_string(),
  )
}

fn paste_text(text: String) -> anyhow::Result<()> {
  let mut clipboard = arboard::Clipboard::new()?;
  clipboard.set_text(text)?;
  std::thread::sleep(Duration::from_millis(50));
  let mut enigo = Enigo::new();
  enigo.key_down(Key::Control);
  enigo.key_click(Key::Layout('v'));
  enigo.key_up(Key::Control);
  Ok(())
}

fn is_startup_launch() -> bool {
  env::args().any(|arg| arg == "--startup")
}

fn main() {
  tauri::Builder::default()
    .setup(|app| {
      let handle = app.handle();
      let settings = load_settings(&handle);

      let state = AppState {
        settings: Arc::new(Mutex::new(settings.clone())),
        recording: Arc::new(AtomicBool::new(false)),
        buffer: Arc::new(Mutex::new(Vec::new())),
        sample_rate: Arc::new(Mutex::new(16_000)),
        stop_tx: Arc::new(Mutex::new(None)),
        active_hotkey: Arc::new(Mutex::new(String::new())),
        hotkey_registered: Arc::new(AtomicBool::new(false)),
        last_hotkey_error: Arc::new(Mutex::new(String::new())),
      };
      app.manage(state);

      let main_window = app.get_window("main").context("Main window missing")?;
      let app_for_main = handle.clone();
      main_window.on_window_event(move |event| {
        if let WindowEvent::CloseRequested { api, .. } = event {
          let bar_hidden = app_for_main.state::<AppState>().settings.lock().unwrap().bar_hidden;
          if !bar_hidden {
            api.prevent_close();
            let _ = app_for_main.get_window("main").map(|window| window.hide());
          }
        }
      });

      let bar = tauri::WindowBuilder::new(app, "bar", tauri::WindowUrl::App("bar.html".into()))
        .decorations(false)
        .transparent(true)
        .always_on_top(true)
        .resizable(false)
        .inner_size(208.0, 52.0)
        .skip_taskbar(true)
        .build()?;

      apply_bar_position(&bar, &settings)?;
      if settings.bar_hidden {
        bar.hide()?;
      }

      let app_for_bar = handle.clone();
      bar.on_window_event(move |event| {
        if let WindowEvent::Moved(position) = event {
          store_bar_position(&app_for_bar, position.x as f64, position.y as f64);
        }
      });

      apply_startup_setting(&settings)?;
      let _ = register_primary_hotkey(&handle);

      if is_startup_launch() {
        let _ = main_window.hide();
      }

      Ok(())
    })
    .invoke_handler(tauri::generate_handler![
      get_settings,
      get_runtime_status,
      save_settings,
      set_bar_hidden,
      hide_bar_and_show_hub,
      start_bar_drag,
      show_hub,
      toggle_recording
    ])
    .run(tauri::generate_context!())
    .expect("error while running tauri application");
}
