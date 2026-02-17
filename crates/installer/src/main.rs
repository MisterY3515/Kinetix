use eframe::egui;
use std::path::{Path, PathBuf};
use std::fs;

#[cfg(target_os = "windows")]
use winreg::enums::*;
#[cfg(target_os = "windows")]
use winreg::RegKey;

// Embed release binaries (platform-specific names)
#[cfg(target_os = "windows")]
const CLI_BYTES: &[u8] = include_bytes!("../../../target/release/kivm.exe");
#[cfg(target_os = "windows")]
const KICOMP_BYTES: &[u8] = include_bytes!("../../../target/release/kicomp.exe");

#[cfg(not(target_os = "windows"))]
const CLI_BYTES: &[u8] = include_bytes!("../../../target/release/kivm");
#[cfg(not(target_os = "windows"))]
const KICOMP_BYTES: &[u8] = include_bytes!("../../../target/release/kicomp");

fn cli_filename() -> &'static str {
    if cfg!(target_os = "windows") { "kivm.exe" } else { "kivm" }
}

fn kicomp_filename() -> &'static str {
    if cfg!(target_os = "windows") { "kicomp.exe" } else { "kicomp" }
}

// ─── Theme colors ──────────────────────────────────────────────────────────
const ACCENT: egui::Color32 = egui::Color32::from_rgb(108, 99, 255);    // #6c63ff
const ACCENT_HOVER: egui::Color32 = egui::Color32::from_rgb(130, 122, 255);
const ACCENT_DIM: egui::Color32 = egui::Color32::from_rgb(78, 71, 200);
const BG_DARK: egui::Color32 = egui::Color32::from_rgb(18, 18, 30);     // #12121e
const BG_PANEL: egui::Color32 = egui::Color32::from_rgb(26, 26, 46);    // #1a1a2e
const BG_CARD: egui::Color32 = egui::Color32::from_rgb(35, 35, 58);     // #23233a
const TEXT_PRIMARY: egui::Color32 = egui::Color32::from_rgb(230, 230, 240);
const TEXT_SECONDARY: egui::Color32 = egui::Color32::from_rgb(140, 140, 165);
const TEXT_DIM: egui::Color32 = egui::Color32::from_rgb(90, 90, 115);
const SUCCESS: egui::Color32 = egui::Color32::from_rgb(80, 200, 120);
const ERROR_COLOR: egui::Color32 = egui::Color32::from_rgb(220, 80, 80);
const BORDER: egui::Color32 = egui::Color32::from_rgb(50, 50, 75);

fn setup_theme(ctx: &egui::Context) {
    let mut style = (*ctx.style()).clone();

    // Visuals
    let v = &mut style.visuals;
    v.dark_mode = true;
    v.override_text_color = Some(TEXT_PRIMARY);

    v.window_fill = BG_DARK;
    v.panel_fill = BG_PANEL;
    v.faint_bg_color = BG_CARD;

    v.window_rounding = egui::Rounding::same(12.0);
    v.window_shadow = egui::epaint::Shadow::NONE;

    // Widget defaults
    v.widgets.noninteractive.bg_fill = BG_CARD;
    v.widgets.noninteractive.fg_stroke = egui::Stroke::new(1.0, TEXT_SECONDARY);
    v.widgets.noninteractive.rounding = egui::Rounding::same(8.0);

    v.widgets.inactive.bg_fill = BG_CARD;
    v.widgets.inactive.fg_stroke = egui::Stroke::new(1.0, TEXT_PRIMARY);
    v.widgets.inactive.rounding = egui::Rounding::same(8.0);
    v.widgets.inactive.bg_stroke = egui::Stroke::new(1.0, BORDER);

    v.widgets.hovered.bg_fill = egui::Color32::from_rgb(45, 45, 72);
    v.widgets.hovered.fg_stroke = egui::Stroke::new(1.0, TEXT_PRIMARY);
    v.widgets.hovered.rounding = egui::Rounding::same(8.0);
    v.widgets.hovered.bg_stroke = egui::Stroke::new(1.0, ACCENT);

    v.widgets.active.bg_fill = ACCENT_DIM;
    v.widgets.active.fg_stroke = egui::Stroke::new(1.0, TEXT_PRIMARY);
    v.widgets.active.rounding = egui::Rounding::same(8.0);

    // Selection
    v.selection.bg_fill = ACCENT.linear_multiply(0.3);
    v.selection.stroke = egui::Stroke::new(1.0, ACCENT);

    // Spacing
    style.spacing.item_spacing = egui::vec2(8.0, 8.0);
    style.spacing.button_padding = egui::vec2(16.0, 8.0);

    ctx.set_style(style);
}

fn main() -> eframe::Result<()> {
    std::panic::set_hook(Box::new(|info| {
        let msg = match info.payload().downcast_ref::<&str>() {
            Some(s) => *s,
            None => match info.payload().downcast_ref::<String>() {
                Some(s) => &s[..],
                None => "Box<Any>",
            },
        };
        let location = info.location()
            .map(|l| format!("{}:{}:{}", l.file(), l.line(), l.column()))
            .unwrap_or_else(|| "unknown".to_string());
        let _ = std::fs::write("crash_log.txt", format!("Panic at {}: {}\n", location, msg));
    }));

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([520.0, 520.0])
            .with_min_inner_size([520.0, 520.0])
            .with_decorations(true),
        ..Default::default()
    };

    eframe::run_native(
        "Kinetix Installer",
        options,
        Box::new(|cc| {
            setup_theme(&cc.egui_ctx);
            Ok(Box::new(InstallerApp::new(cc)))
        }),
    )
}

// ─── App State ─────────────────────────────────────────────────────────────

#[derive(PartialEq)]
enum InstallState {
    Ready,
    Installing,
    Done,
    Failed(String),
}

struct InstallerApp {
    install_path: PathBuf,
    install_kivm: bool,
    install_kicomp: bool,
    add_to_path: bool,
    state: InstallState,
}

impl InstallerApp {
    fn new(_cc: &eframe::CreationContext<'_>) -> Self {
        Self {
            install_path: default_install_path(),
            install_kivm: true,
            install_kicomp: true,
            add_to_path: true,
            state: InstallState::Ready,
        }
    }

    fn install(&mut self) {
        self.state = InstallState::Installing;
        match self.perform_install() {
            Ok(()) => self.state = InstallState::Done,
            Err(e) => self.state = InstallState::Failed(format!("{}", e)),
        }
    }

    fn perform_install(&mut self) -> std::io::Result<()> {
        fs::create_dir_all(&self.install_path)?;

        let bin_dir = self.install_path.join("bin");
        fs::create_dir_all(&bin_dir)?;

        let cli_path = bin_dir.join(cli_filename());
        let comp_path = bin_dir.join(kicomp_filename());

        if self.install_kivm {
            fs::write(&cli_path, CLI_BYTES)?;
            #[cfg(unix)]
            set_executable(&cli_path)?;
        }

        if self.install_kicomp {
            fs::write(&comp_path, KICOMP_BYTES)?;
            #[cfg(unix)]
            set_executable(&comp_path)?;
        }

        #[cfg(target_os = "windows")]
        {
            if self.add_to_path {
                add_to_user_path_win(&bin_dir)?;
            }
            let exe = cli_path.to_str().unwrap();
            let comp_exe = comp_path.to_str().unwrap();
            if self.install_kivm {
                register_progid(".exki", "Kinetix.Bundle")?;
                register_shell("Kinetix.Bundle", "Kinetix Bundle", exe, "run")?;
                register_progid(".kix", "Kinetix.Source")?;
                register_progid(".ki", "Kinetix.Source")?;
                register_shell("Kinetix.Source", "Kinetix Source File", exe, "exec")?;
            }
            if self.install_kicomp {
                register_progid(".kicomp", "Kinetix.Build")?;
                register_shell("Kinetix.Build", "Kinetix Build Script", comp_exe, "")?;
            }
        }

        #[cfg(target_os = "linux")]
        {
            if self.add_to_path { add_to_path_unix(&bin_dir)?; }
            create_desktop_entry(&self.install_path)?;
        }

        #[cfg(target_os = "macos")]
        {
            if self.add_to_path { add_to_path_unix(&bin_dir)?; }
        }

        Ok(())
    }
}

impl eframe::App for InstallerApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        egui::CentralPanel::default()
            .frame(egui::Frame::none().fill(BG_DARK).inner_margin(32.0))
            .show(ctx, |ui| {
                // ── Header ──
                ui.vertical_centered(|ui| {
                    ui.add_space(4.0);
                    ui.label(
                        egui::RichText::new("KINETIX")
                            .size(28.0)
                            .color(TEXT_PRIMARY)
                            .strong()
                    );
                    ui.add_space(2.0);
                    ui.label(
                        egui::RichText::new(format!(
                            "v{}  •  {} ({})",
                            env!("CARGO_PKG_VERSION"),
                            std::env::consts::OS,
                            std::env::consts::ARCH
                        ))
                        .size(12.0)
                        .color(TEXT_DIM)
                    );

                    // Accent line
                    ui.add_space(8.0);
                    let rect = ui.available_rect_before_wrap();
                    let line_rect = egui::Rect::from_min_size(
                        egui::pos2(rect.left() + 60.0, rect.top()),
                        egui::vec2(rect.width() - 120.0, 2.0),
                    );
                    ui.painter().rect_filled(line_rect, 1.0, ACCENT);
                    ui.add_space(12.0);
                });

                // ── Components card ──
                ui.add_space(8.0);
                egui::Frame::none()
                    .fill(BG_CARD)
                    .rounding(10.0)
                    .inner_margin(16.0)
                    .stroke(egui::Stroke::new(1.0, BORDER))
                    .show(ui, |ui| {
                        ui.label(
                            egui::RichText::new("Components")
                                .size(13.0)
                                .color(TEXT_SECONDARY)
                                .strong()
                        );
                        ui.add_space(6.0);
                        ui.checkbox(&mut self.install_kivm, egui::RichText::new("KiVM — Interpreter & CLI").size(14.0));
                        ui.checkbox(&mut self.install_kicomp, egui::RichText::new("KiComp — Compiler & Build System").size(14.0));
                    });

                // ── Integration card ──
                ui.add_space(8.0);
                egui::Frame::none()
                    .fill(BG_CARD)
                    .rounding(10.0)
                    .inner_margin(16.0)
                    .stroke(egui::Stroke::new(1.0, BORDER))
                    .show(ui, |ui| {
                        ui.label(
                            egui::RichText::new("System Integration")
                                .size(13.0)
                                .color(TEXT_SECONDARY)
                                .strong()
                        );
                        ui.add_space(6.0);
                        ui.checkbox(&mut self.add_to_path, egui::RichText::new("Add to user PATH").size(14.0));
                    });

                // ── Install path ──
                ui.add_space(8.0);
                egui::Frame::none()
                    .fill(BG_CARD)
                    .rounding(10.0)
                    .inner_margin(16.0)
                    .stroke(egui::Stroke::new(1.0, BORDER))
                    .show(ui, |ui| {
                        ui.label(
                            egui::RichText::new("Install Location")
                                .size(13.0)
                                .color(TEXT_SECONDARY)
                                .strong()
                        );
                        ui.add_space(6.0);
                        ui.horizontal(|ui| {
                            ui.label(
                                egui::RichText::new(self.install_path.to_string_lossy())
                                    .size(13.0)
                                    .color(TEXT_PRIMARY)
                                    .monospace()
                            );
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                if ui.button(egui::RichText::new("Browse").size(12.0)).clicked() {
                                    if let Some(path) = rfd::FileDialog::new().pick_folder() {
                                        self.install_path = path;
                                    }
                                }
                            });
                        });
                    });

                // ── Action / Status ──
                ui.add_space(16.0);

                match &self.state {
                    InstallState::Ready => {
                        ui.vertical_centered(|ui| {
                            let btn = egui::Button::new(
                                egui::RichText::new("  INSTALL  ")
                                    .size(16.0)
                                    .strong()
                                    .color(egui::Color32::WHITE)
                            )
                            .fill(ACCENT)
                            .rounding(10.0)
                            .min_size(egui::vec2(200.0, 44.0));

                            if ui.add(btn).clicked() {
                                self.install();
                            }
                        });
                    }
                    InstallState::Installing => {
                        ui.vertical_centered(|ui| {
                            ui.spinner();
                            ui.add_space(4.0);
                            ui.label(
                                egui::RichText::new("Installing...")
                                    .size(14.0)
                                    .color(TEXT_SECONDARY)
                            );
                        });
                    }
                    InstallState::Done => {
                        ui.vertical_centered(|ui| {
                            ui.label(
                                egui::RichText::new("✓  Installation complete")
                                    .size(16.0)
                                    .color(SUCCESS)
                                    .strong()
                            );
                            ui.add_space(4.0);
                            ui.label(
                                egui::RichText::new("Restart your terminal to use kivm.")
                                    .size(13.0)
                                    .color(TEXT_SECONDARY)
                            );
                        });
                    }
                    InstallState::Failed(msg) => {
                        ui.vertical_centered(|ui| {
                            ui.label(
                                egui::RichText::new("✗  Installation failed")
                                    .size(16.0)
                                    .color(ERROR_COLOR)
                                    .strong()
                            );
                            ui.add_space(4.0);
                            ui.label(
                                egui::RichText::new(msg)
                                    .size(12.0)
                                    .color(TEXT_SECONDARY)
                                    .monospace()
                            );
                        });
                    }
                }

                // ── Footer ──
                ui.with_layout(egui::Layout::bottom_up(egui::Align::Center), |ui| {
                    ui.add_space(4.0);
                    ui.label(
                        egui::RichText::new("© 2026 MisterY3515")
                            .size(11.0)
                            .color(TEXT_DIM)
                    );
                });
            });
    }
}

// ─── Cross-platform helpers ────────────────────────────────────────────────

fn default_install_path() -> PathBuf {
    if let Some(dirs) = directories::BaseDirs::new() {
        dirs.home_dir().join(".kinetix")
    } else if cfg!(target_os = "windows") {
        PathBuf::from("C:\\Kinetix")
    } else {
        PathBuf::from("/opt/kinetix")
    }
}

#[cfg(unix)]
fn set_executable(path: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let mut perms = fs::metadata(path)?.permissions();
    perms.set_mode(0o755);
    fs::set_permissions(path, perms)
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn add_to_path_unix(bin_dir: &Path) -> std::io::Result<()> {
    let home = std::env::var("HOME").unwrap_or_default();
    let bin_str = bin_dir.to_string_lossy();
    for profile in &[".bashrc", ".zshrc", ".profile"] {
        let profile_path = PathBuf::from(&home).join(profile);
        if profile_path.exists() {
            let content = fs::read_to_string(&profile_path)?;
            if !content.contains(&bin_str.to_string()) {
                let mut file = std::fs::OpenOptions::new().append(true).open(&profile_path)?;
                use std::io::Write;
                writeln!(file, "\n# Kinetix\nexport PATH=\"{}:$PATH\"", bin_str)?;
            }
        }
    }
    Ok(())
}

#[cfg(target_os = "linux")]
fn create_desktop_entry(install_path: &Path) -> std::io::Result<()> {
    let home = std::env::var("HOME").unwrap_or_default();
    let apps_dir = PathBuf::from(&home).join(".local/share/applications");
    fs::create_dir_all(&apps_dir)?;
    fs::write(
        apps_dir.join("kinetix.desktop"),
        format!(
            "[Desktop Entry]\nName=Kinetix\nComment=Kinetix Language Runtime\nExec={}/bin/kivm exec %f\nTerminal=true\nType=Application\nCategories=Development;\nMimeType=text/x-kinetix;\n",
            install_path.display()
        ),
    )?;
    Ok(())
}

// ─── Windows-only ──────────────────────────────────────────────────────────

#[cfg(target_os = "windows")]
fn add_to_user_path_win(path: &Path) -> std::io::Result<()> {
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let env = hkcu.open_subkey_with_flags("Environment", KEY_READ | KEY_WRITE)?;
    let current_path: String = env.get_value("Path")?;
    let path_str = path.to_str()
        .ok_or(std::io::Error::new(std::io::ErrorKind::InvalidData, "Invalid path"))?;
    if !current_path.contains(path_str) {
        let sep = if current_path.ends_with(';') { "" } else { ";" };
        env.set_value("Path", &format!("{}{}{}", current_path, sep, path_str))?;
    }
    Ok(())
}

#[cfg(target_os = "windows")]
fn register_progid(ext: &str, prog_id: &str) -> std::io::Result<()> {
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let classes = hkcu.open_subkey_with_flags("Software\\Classes", KEY_READ | KEY_WRITE)?;
    let (key, _) = classes.create_subkey(ext)?;
    key.set_value("", &prog_id)?;
    Ok(())
}

#[cfg(target_os = "windows")]
fn register_shell(prog_id: &str, desc: &str, exe: &str, arg: &str) -> std::io::Result<()> {
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let classes = hkcu.open_subkey_with_flags("Software\\Classes", KEY_READ | KEY_WRITE)?;
    let (prog_key, _) = classes.create_subkey(prog_id)?;
    prog_key.set_value("", &desc)?;
    let (icon_key, _) = prog_key.create_subkey("DefaultIcon")?;
    icon_key.set_value("", &format!("{},0", exe))?;
    let (cmd_key, _) = prog_key.create_subkey("shell\\open\\command")?;
    let cmd = if arg.is_empty() {
        format!("\"{}\" \"%1\"", exe)
    } else {
        format!("\"{}\" {} \"%1\"", exe, arg)
    };
    cmd_key.set_value("", &cmd)?;
    Ok(())
}
