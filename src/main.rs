#![windows_subsystem = "windows"]

use encoding_rs::{UTF_16BE, UTF_16LE, UTF_8};
use iced::widget::{button, column, container, progress_bar, row, text};
use iced::Theme;
use iced::{Alignment, Element, Length, Task};
use quick_xml::events::Event;
use quick_xml::reader::Reader;
use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;

// ============================================================================
// HELPER FUNCTIONS
// ============================================================================

/// Queries the OS (Windows) to determine if Dark Mode is active
fn get_system_theme() -> Theme {
    match dark_light::detect() {
        dark_light::Mode::Dark => Theme::Dark,
        dark_light::Mode::Light => Theme::Light,
        // Fallback to Dark if detection is indeterminate
        dark_light::Mode::Default => Theme::Dark,
    }
}

fn open_folder_in_explorer(path: &Path) {
    let target_dir = if path.is_file() {
        path.parent().unwrap_or(path)
    } else {
        path
    };

    let _ = Command::new("explorer").arg(target_dir).spawn();
}

// ============================================================================
// APPLICATION ENTRY POINT
// ============================================================================

fn main() -> iced::Result {
    // Using iced::application allows passing the theme hook
    iced::application("XML Song Extractor", App::update, App::view)
        .theme(App::theme)
        .run()
}

// ============================================================================
// STATE & TYPES
// ============================================================================

#[derive(Clone, Debug)]
enum SelectedSource {
    None,
    Files(Vec<PathBuf>),
    Folder(PathBuf),
}

struct App {
    source: SelectedSource,
    is_processing: bool,
    progress: f32,
    total_files: usize,
    status_message: String,
    theme: Theme,
}

impl Default for App {
    fn default() -> Self {
        Self {
            source: SelectedSource::None,
            is_processing: false,
            progress: 0.0,
            total_files: 0,
            status_message: String::from("Select files or a folder to get started."),
            theme: get_system_theme(), // Sets the theme once on app launch
        }
    }
}

/// Updated Messages to support non-blocking asynchronous dialogs
#[derive(Debug, Clone)]
enum Message {
    SelectFiles,
    FilesSelected(Option<Vec<PathBuf>>),
    SelectFolder,
    FolderSelected(Option<PathBuf>),
    StartProcessing,
    BatchFinished {
        success_count: usize,
        error_count: usize,
        log_created: bool,
    },
    OpenOutputFolder,
    DialogClosed,
}

// ============================================================================
// APP LOGIC & UI RENDERING
// ============================================================================

impl App {
    fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            // Async File Dialog Trigger
            Message::SelectFiles => Task::perform(
                async {
                    rfd::AsyncFileDialog::new()
                        .add_filter("XML Files", &["xml"])
                        .pick_files()
                        .await
                        .map(|handles| handles.into_iter().map(|h| h.path().to_path_buf()).collect())
                },
                Message::FilesSelected,
            ),

            // Receive selected files non-blockingly
            Message::FilesSelected(files) => {
                if let Some(files) = files {
                    self.total_files = files.len();
                    self.status_message = format!("{} file(s) selected.", self.total_files);
                    self.source = SelectedSource::Files(files);
                }
                Task::none()
            }

            // Async Folder Dialog Trigger
            Message::SelectFolder => Task::perform(
                async {
                    rfd::AsyncFileDialog::new()
                        .pick_folder()
                        .await
                        .map(|handle| handle.path().to_path_buf())
                },
                Message::FolderSelected,
            ),

            // Receive selected folder non-blockingly
            Message::FolderSelected(folder) => {
                if let Some(folder) = folder {
                    let xml_count = count_xml_in_folder(&folder);
                    self.total_files = xml_count;
                    self.status_message = format!("Folder selected ({} XML files found).", xml_count);
                    self.source = SelectedSource::Folder(folder);
                }
                Task::none()
            }

            // Start Extraction Async Task
            Message::StartProcessing => {
                if self.total_files == 0 {
                    self.status_message = String::from("No XML files to process!");
                    return Task::none();
                }

                self.is_processing = true;
                self.progress = 0.0;
                self.status_message = String::from("Processing files...");

                let source = self.source.clone();

                Task::perform(
                    async move {
                        let (targets, export_dir) = match source {
                            SelectedSource::Files(ref files) => {
                                let dir = files
                                    .first()
                                    .and_then(|f| f.parent())
                                    .map(|p| p.to_path_buf())
                                    .unwrap_or_else(|| PathBuf::from("."));
                                (files.clone(), dir)
                            }
                            SelectedSource::Folder(ref dir) => {
                                (get_xml_files_in_folder(dir), dir.clone())
                            }
                            SelectedSource::None => (vec![], PathBuf::from(".")),
                        };

                        let mut success_count = 0;
                        let mut errors: Vec<String> = Vec::new();

                        for path in &targets {
                            let out_path = path.with_extension("txt");
                            match extract_song_data(path, &out_path) {
                                Ok(_) => success_count += 1,
                                Err(err) => {
                                    let filename =
                                        path.file_name().unwrap_or_default().to_string_lossy();
                                    errors.push(format!("[{}] {}", filename, err));
                                }
                            }
                        }

                        let mut log_created = false;
                        if !errors.is_empty() {
                            let log_path = export_dir.join("error_log.txt");
                            if let Ok(mut log_file) = File::create(&log_path) {
                                let _ = writeln!(log_file, "--- XML Extractor Error Log ---");
                                for err in &errors {
                                    let _ = writeln!(log_file, "{}", err);
                                }
                                log_created = true;
                            }
                        }

                        (success_count, errors.len(), log_created)
                    },
                    |(success_count, error_count, log_created)| Message::BatchFinished {
                        success_count,
                        error_count,
                        log_created,
                    },
                )
            }

            // Handle Completion and trigger Non-Blocking Async Message Dialog
            Message::BatchFinished {
                success_count,
                error_count,
                log_created,
            } => {
                self.is_processing = false;
                self.progress = 1.0;

                let mut result_msg = format!(
                    "Finished! Processed {}/{} files successfully.",
                    success_count, self.total_files
                );

                if error_count > 0 {
                    result_msg.push_str(&format!(" {} file(s) failed.", error_count));
                    if log_created {
                        result_msg.push_str(" See error_log.txt for details.");
                    }
                }

                self.status_message = result_msg.clone();

                let popup_title = if error_count == 0 {
                    "Success".to_string()
                } else {
                    "Completed with Warnings".to_string()
                };

                // Spawn non-blocking modal popup on background thread
                Task::perform(
                    async move {
                        rfd::AsyncMessageDialog::new()
                            .set_title(&popup_title)
                            .set_description(&result_msg)
                            .show()
                            .await;
                    },
                    |_| Message::DialogClosed,
                )
            }

            Message::OpenOutputFolder => {
                let target_dir = match &self.source {
                    SelectedSource::Files(files) => files
                        .first()
                        .and_then(|f| f.parent())
                        .map(|p| p.to_path_buf())
                        .unwrap_or_else(|| PathBuf::from(".")),
                    SelectedSource::Folder(dir) => dir.clone(),
                    SelectedSource::None => PathBuf::from("."),
                };

                open_folder_in_explorer(&target_dir);
                Task::none()
            }

            Message::DialogClosed => Task::none(),
        }
    }

    fn view(&self) -> Element<'_, Message> {
        let title = text("XML Song Extractor").size(28);

        let btn_files = button("Choose Files")
            .on_press_maybe((!self.is_processing).then_some(Message::SelectFiles));
        let btn_folder = button("Choose Folder")
            .on_press_maybe((!self.is_processing).then_some(Message::SelectFolder));

        let has_source = !matches!(self.source, SelectedSource::None);
        let btn_open_dir = button("Open Folder").on_press_maybe(
            (has_source && !self.is_processing).then_some(Message::OpenOutputFolder),
        );

        let picker_row = row![btn_files, btn_folder, btn_open_dir]
            .spacing(15)
            .align_y(Alignment::Center);

        let can_start = !self.is_processing && self.total_files > 0;
        let btn_start = button("Start Extraction")
            .on_press_maybe(can_start.then_some(Message::StartProcessing));

        let status_text = text(&self.status_message).size(16);

        let mut content = column![title, picker_row, btn_start, status_text]
            .spacing(20)
            .align_x(Alignment::Center);

        if self.is_processing {
            content = content.push(progress_bar(0.0..=1.0, self.progress));
        }

        container(content)
            .width(Length::Fill)
            .height(Length::Fill)
            .center_x(Length::Fill)
            .center_y(Length::Fill)
            .padding(20)
            .into()
    }

    // Tell Iced to use the system theme detected on startup
    fn theme(&self) -> Theme {
        self.theme.clone()
    }
}

// ============================================================================
// XML PARSING LOGIC
// ============================================================================

fn extract_song_data(input_xml: &Path, output_txt: &Path) -> Result<(), String> {
    let raw_bytes = fs::read(input_xml).map_err(|e| format!("Failed to read file: {}", e))?;

    let (decoded_str, _, _) = if raw_bytes.starts_with(&[0xFF, 0xFE]) {
        UTF_16LE.decode(&raw_bytes[2..])
    } else if raw_bytes.starts_with(&[0xFE, 0xFF]) {
        UTF_16BE.decode(&raw_bytes[2..])
    } else {
        UTF_8.decode(&raw_bytes)
    };

    let mut reader = Reader::from_str(&decoded_str);
    reader.config_mut().trim_text(true);

    let mut current_count: Option<String> = None;
    let mut written_entries = 0;
    let mut out_file =
        File::create(output_txt).map_err(|e| format!("Failed to create TXT output: {}", e))?;

    let mut buf = Vec::new();
    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(e)) | Ok(Event::Empty(e)) if e.name().as_ref() == b"Entry" => {
                current_count = None;
                for attr in e.attributes().flatten() {
                    if attr.key.as_ref() == b"Count" {
                        let count_val = String::from_utf8_lossy(&attr.value).trim().to_string();
                        if let Ok(num) = count_val.parse::<i32>() {
                            if num > 0 {
                                current_count = Some(count_val);
                            }
                        }
                    }
                }
            }
            Ok(Event::Start(e)) | Ok(Event::Empty(e)) if e.name().as_ref() == b"Item" => {
                if let Some(count_str) = &current_count {
                    for attr in e.attributes().flatten() {
                        if attr.key.as_ref() == b"Path" {
                            let path_str = String::from_utf8_lossy(&attr.value);

                            let song_name = Path::new(path_str.as_ref())
                                .file_name()
                                .and_then(|s| s.to_str())
                                .unwrap_or_default();

                            writeln!(out_file, "Song: {}\nCount: {}\n", song_name, count_str)
                                .map_err(|e| format!("Write failed: {}", e))?;
                            written_entries += 1;
                        }
                    }
                }
            }
            Ok(Event::Eof) => break,
            Err(e) => return Err(format!("XML Parse Error: {}", e)),
            _ => (),
        }
        buf.clear();
    }

    if written_entries == 0 {
        let _ = fs::remove_file(output_txt);
        return Err(String::from("No valid <Entry> tags with Count > 0 found."));
    }

    Ok(())
}

fn count_xml_in_folder(dir: &Path) -> usize {
    get_xml_files_in_folder(dir).len()
}

fn get_xml_files_in_folder(dir: &Path) -> Vec<PathBuf> {
    fs::read_dir(dir)
        .ok()
        .map(|entries| {
            entries
                .flatten()
                .map(|e| e.path())
                .filter(|p| p.is_file() && p.extension().map_or(false, |ext| ext == "xml"))
                .collect()
        })
        .unwrap_or_default()
}