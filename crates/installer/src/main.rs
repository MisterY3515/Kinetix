#![windows_subsystem = "windows"]

use eframe::egui;
use std::path::{Path, PathBuf};
use std::fs;

#[cfg(target_os = "windows")]
use winreg::enums::*;
#[cfg(target_os = "windows")]
use winreg::RegKey;

// ─── Embedded binaries ─────────────────────────────────────────────────────
#[cfg(target_os = "windows")]
const CLI_BYTES: &[u8] = include_bytes!("../../../target/release/kivm.exe");
#[cfg(target_os = "windows")]
const KICOMP_BYTES: &[u8] = include_bytes!("../../../target/release/kicomp.exe");

#[cfg(not(target_os = "windows"))]
const CLI_BYTES: &[u8] = include_bytes!("../../../target/release/kivm");
#[cfg(not(target_os = "windows"))]
const KICOMP_BYTES: &[u8] = include_bytes!("../../../target/release/kicomp");

// ─── Embedded assets ───────────────────────────────────────────────────────
const ICON_BYTES: &[u8] = include_bytes!("../../../assets/logo/KiFile.png");

// NOTE: Documentation is embedded via build_installer script which creates
// a docs.tar file. If it doesn't exist at compile time, docs won't be available.
// For now, docs are copied from the Documentation/ folder by the installer build script.

fn cli_filename() -> &'static str {
    if cfg!(target_os = "windows") { "kivm.exe" } else { "kivm" }
}

fn kicomp_filename() -> &'static str {
    if cfg!(target_os = "windows") { "kicomp.exe" } else { "kicomp" }
}

// ─── Theme ─────────────────────────────────────────────────────────────────
const ACCENT: egui::Color32 = egui::Color32::from_rgb(108, 99, 255);
const ACCENT_HOVER: egui::Color32 = egui::Color32::from_rgb(130, 122, 255);
const ACCENT_DIM: egui::Color32 = egui::Color32::from_rgb(78, 71, 200);
const BG_DARK: egui::Color32 = egui::Color32::from_rgb(18, 18, 30);
const BG_PANEL: egui::Color32 = egui::Color32::from_rgb(26, 26, 46);
const BG_CARD: egui::Color32 = egui::Color32::from_rgb(35, 35, 58);
const TEXT_PRIMARY: egui::Color32 = egui::Color32::from_rgb(230, 230, 240);
const TEXT_SECONDARY: egui::Color32 = egui::Color32::from_rgb(140, 140, 165);
const TEXT_DIM: egui::Color32 = egui::Color32::from_rgb(90, 90, 115);
const SUCCESS: egui::Color32 = egui::Color32::from_rgb(80, 200, 120);
const ERROR_COLOR: egui::Color32 = egui::Color32::from_rgb(220, 80, 80);
const BORDER: egui::Color32 = egui::Color32::from_rgb(50, 50, 75);
const PROGRESS_BG: egui::Color32 = egui::Color32::from_rgb(40, 40, 65);

fn setup_theme(ctx: &egui::Context) {
    let mut style = (*ctx.style()).clone();
    let v = &mut style.visuals;
    v.dark_mode = true;
    v.override_text_color = Some(TEXT_PRIMARY);
    v.window_fill = BG_DARK;
    v.panel_fill = BG_PANEL;
    v.faint_bg_color = BG_CARD;
    v.window_rounding = egui::Rounding::same(12.0);
    v.window_shadow = egui::epaint::Shadow::NONE;

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
    v.selection.bg_fill = ACCENT.linear_multiply(0.3);
    v.selection.stroke = egui::Stroke::new(1.0, ACCENT);

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
            .with_inner_size([520.0, 580.0])
            .with_min_inner_size([520.0, 580.0])
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

// ─── Install Steps ─────────────────────────────────────────────────────────

const STEP_NAMES: &[&str] = &[
    "Creating directories",
    "Installing KiVM",
    "Installing KiComp",
    "Installing icon",
    "Configuring PATH",
    "Setting file associations",
    "Installing documentation",
    "Finalizing",
];

// ─── App State ─────────────────────────────────────────────────────────────

#[derive(PartialEq)]
enum InstallState {
    Ready,
    Installing { step: usize, total: usize },
    Done,
    Failed(String),
}

struct InstallerApp {
    install_path: PathBuf,
    install_kivm: bool,
    install_kicomp: bool,
    install_docs: bool,
    add_to_path: bool,
    state: InstallState,
}

impl InstallerApp {
    fn new(_cc: &eframe::CreationContext<'_>) -> Self {
        Self {
            install_path: default_install_path(),
            install_kivm: true,
            install_kicomp: true,
            install_docs: true,
            add_to_path: true,
            state: InstallState::Ready,
        }
    }

    fn log(&mut self, msg: impl Into<String>) {
        let s = msg.into();
        println!("{}", s); // Only print to stdout
    }

    fn install(&mut self) {
        let total = STEP_NAMES.len();
        let mut current_step = 0;

        macro_rules! step {
            ($name:expr, $body:expr) => {
                let name = $name;
                self.log(format!("Starting step: {}", name));
                self.state = InstallState::Installing { step: current_step, total };
                current_step += 1;
                match (|| -> std::io::Result<()> { $body; Ok(()) })() {
                    Ok(_) => {
                        self.log(format!("✓ Step complete: {}", name));
                    }
                    Err(e) => {
                        let err_msg = format!("Failed: {}", e);
                        self.log(&err_msg);
                        self.state = InstallState::Failed(format!("{}: {}", name, e));
                        return;
                    }
                }
            };
        }

        // Step 0: Create directories
        step!("Create directories", {
            fs::create_dir_all(&self.install_path)?;
            fs::create_dir_all(self.install_path.join("bin"))?;
            fs::create_dir_all(self.install_path.join("assets"))?;
            // Clean up old root-level binaries from previous installations
            let old_cli = self.install_path.join(cli_filename());
            let old_comp = self.install_path.join(kicomp_filename());
            if old_cli.exists() {
                self.log(format!("Removing old root-level {:?}", old_cli));
                let _ = fs::remove_file(&old_cli);
            }
            if old_comp.exists() {
                self.log(format!("Removing old root-level {:?}", old_comp));
                let _ = fs::remove_file(&old_comp);
            }
        });

        // Diagnostic: Check for existing installations
        step!("Check Conflicts", {
            self.log("Scanning PATH for existing kivm installations...");
            if let Some(path_var) = std::env::var_os("PATH") {
                let target_bin = self.install_path.join("bin").join(cli_filename());
                for path in std::env::split_paths(&path_var) {
                    let exe = path.join(cli_filename());
                    if exe.exists() {
                        if exe != target_bin {
                            self.log(format!("⚠️ Found existing kivm at: {:?}", exe));
                            self.log("   (This might be taking precedence over the new install)");
                        } else {
                            self.log(format!("Found previous install at target: {:?}", exe));
                        }
                    }
                }
            } else {
                self.log("Warning: Could not read PATH environment variable.");
            }
        });

        let bin_dir = self.install_path.join("bin");

        // Step 1: Install KiVM
        step!("Install KiVM", {
            if self.install_kivm {
                let cli_path = bin_dir.join(cli_filename());
                self.log(format!("Writing kivm to {:?}", cli_path));
                fs::write(&cli_path, CLI_BYTES)?;
                #[cfg(unix)]
                set_executable(&cli_path)?;
            } else {
                self.log("Skipping KiVM installation");
            }
        });

        // Step 2: Install KiComp
        step!("Install KiComp", {
            if self.install_kicomp {
                let comp_path = bin_dir.join(kicomp_filename());
                self.log(format!("Writing kicomp to {:?}", comp_path));
                fs::write(&comp_path, KICOMP_BYTES)?;
                #[cfg(unix)]
                set_executable(&comp_path)?;
            } else {
                self.log("Skipping KiComp installation");
            }
        });

        // Step 3: Install icon
        step!("Install Icon", {
            let icon_path = self.install_path.join("assets").join("KiFile.png");
            self.log(format!("Writing icon to {:?}", icon_path));
            fs::write(&icon_path, ICON_BYTES)
        });

        // Step 4: Configure PATH
        step!("Configure PATH", {
            if self.add_to_path {
                self.log("Adding to PATH...");
                #[cfg(target_os = "windows")]
                add_to_user_path_win(&bin_dir)?;
                #[cfg(any(target_os = "linux", target_os = "macos"))]
                add_to_path_unix(&bin_dir)?;
            } else {
                self.log("Skipping PATH configuration");
            }
        });

        // Step 5: File associations
        step!("File Associations", {
            #[cfg(target_os = "windows")]
            {
                let icon_path = self.install_path.join("assets").join("KiFile.png");
                let icon_str = icon_path.to_string_lossy().to_string();
                let cli_path = bin_dir.join(cli_filename());
                let comp_path = bin_dir.join(kicomp_filename());
                let exe = cli_path.to_str().unwrap();
                let comp_exe = comp_path.to_str().unwrap();

                if self.install_kivm {
                    self.log("Registering .exki, .kix, .ki associations...");
                    register_progid(".exki", "Kinetix.Bundle")?;
                    register_shell("Kinetix.Bundle", "Kinetix Bundle", exe, "run", &icon_str)?;
                    register_progid(".kix", "Kinetix.Source")?;
                    register_progid(".ki", "Kinetix.Source")?;
                    register_shell("Kinetix.Source", "Kinetix Source File", exe, "exec", &icon_str)?;
                }
                if self.install_kicomp {
                    self.log("Registering .kicomp association...");
                    register_progid(".kicomp", "Kinetix.Build")?;
                    register_shell("Kinetix.Build", "Kinetix Build Script", comp_exe, "", &icon_str)?;
                }
            }
            #[cfg(target_os = "linux")]
            create_desktop_entry(&self.install_path)?;
        });

        // Step 6: Install documentation
        step!("Install Documentation", {
            if self.install_docs {
                // Copy docs from the embedded Documentation/ folder
                let exe_dir = std::env::current_exe()
                    .ok()
                    .and_then(|p| p.parent().map(|p| p.to_path_buf()));
                
                let docs_dest = self.install_path.join("docs");
                self.log(format!("Copying docs to {:?}", docs_dest));
                fs::create_dir_all(&docs_dest)?;

                // Try to find docs next to the installer
                if let Some(exe_parent) = &exe_dir {
                    let docs_src = exe_parent.join("docs");
                    if docs_src.exists() {
                        self.log(format!("Found docs at {:?}", docs_src));
                        copy_dir_recursive(&docs_src, &docs_dest)?;
                    } else {
                        // Also try parent/Documentation
                        let docs_src2 = exe_parent.parent()
                            .map(|p| p.join("Documentation"));
                        if let Some(ref src2) = docs_src2 {
                            if src2.exists() {
                                self.log(format!("Found docs at {:?}", src2));
                                copy_dir_recursive(src2, &docs_dest)?;
                            } else {
                                self.log("Warning: Documentation folder not found.");
                            }
                        } else {
                             self.log("Warning: Could not determine Documentation path.");
                        }
                    }
                }
            } else {
                self.log("Skipping documentation");
            }
        });

        // Step 7: Finalize
        step!("Finalizing", { self.log("Cleanup..."); });

        self.state = InstallState::Done;
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
                            "v{} ({})  •  {} ({})",
                            env!("CARGO_PKG_VERSION"),
                            option_env!("KINETIX_BUILD").unwrap_or("Dev"),
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

                match &self.state {
                    InstallState::Ready => self.draw_config(ui),
                    InstallState::Installing { step, total } => {
                        let step = *step;
                        let total = *total;
                        self.draw_progress(ui, step, total);
                    }
                    InstallState::Done => self.draw_done(ui),
                    InstallState::Failed(msg) => {
                        let msg = msg.clone();
                        self.draw_failed(ui, &msg);
                    }
                }

                // ── Footer ──
                ui.with_layout(egui::Layout::bottom_up(egui::Align::Center), |ui| {
                    ui.add_space(8.0);
                    ui.label(
                        egui::RichText::new("© 2026 MisterY3515")
                            .size(11.0)
                            .color(TEXT_DIM)
                    );
                });
            });
    }
}

impl InstallerApp {
    fn draw_config(&mut self, ui: &mut egui::Ui) {
        // Components card
        ui.add_space(4.0);
        egui::Frame::none()
            .fill(BG_CARD)
            .rounding(10.0)
            .inner_margin(16.0)
            .stroke(egui::Stroke::new(1.0, BORDER))
            .show(ui, |ui| {
                ui.label(egui::RichText::new("Components").size(13.0).color(TEXT_SECONDARY).strong());
                ui.add_space(6.0);
                ui.checkbox(&mut self.install_kivm, egui::RichText::new("KiVM — Interpreter, CLI & Shell").size(14.0));
                ui.checkbox(&mut self.install_kicomp, egui::RichText::new("KiComp — Compiler & Build System").size(14.0));
                ui.checkbox(&mut self.install_docs, egui::RichText::new("Documentation (offline, opens with kivm docs)").size(14.0));
            });

        // Integration card
        ui.add_space(8.0);
        egui::Frame::none()
            .fill(BG_CARD)
            .rounding(10.0)
            .inner_margin(16.0)
            .stroke(egui::Stroke::new(1.0, BORDER))
            .show(ui, |ui| {
                ui.label(egui::RichText::new("System Integration").size(13.0).color(TEXT_SECONDARY).strong());
                ui.add_space(6.0);
                ui.checkbox(&mut self.add_to_path, egui::RichText::new("Add to user PATH").size(14.0));
            });

        // Install path
        ui.add_space(8.0);
        egui::Frame::none()
            .fill(BG_CARD)
            .rounding(10.0)
            .inner_margin(16.0)
            .stroke(egui::Stroke::new(1.0, BORDER))
            .show(ui, |ui| {
                ui.label(egui::RichText::new("Install Location").size(13.0).color(TEXT_SECONDARY).strong());
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

        // Install button
        ui.add_space(16.0);
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

    fn draw_progress(&self, ui: &mut egui::Ui, step: usize, total: usize) {
        ui.add_space(30.0);
        ui.vertical_centered(|ui| {
            let step_name = STEP_NAMES.get(step).unwrap_or(&"Working...");
            ui.label(
                egui::RichText::new(format!("{}...", step_name))
                    .size(16.0)
                    .color(TEXT_PRIMARY)
                    .strong()
            );

            ui.add_space(16.0);

            // Progress bar
            let progress = step as f32 / total as f32;
            let bar_width = 360.0;
            let bar_height = 12.0;

            let (rect, _) = ui.allocate_exact_size(
                egui::vec2(bar_width, bar_height),
                egui::Sense::hover(),
            );

            // Background
            ui.painter().rect_filled(rect, 6.0, PROGRESS_BG);

            // Fill
            let fill_rect = egui::Rect::from_min_size(
                rect.min,
                egui::vec2(bar_width * progress, bar_height),
            );
            ui.painter().rect_filled(fill_rect, 6.0, ACCENT);

            ui.add_space(12.0);
            ui.label(
                egui::RichText::new(format!("{} / {}", step + 1, total))
                    .size(13.0)
                    .color(TEXT_SECONDARY)
            );
        });
    }

    fn draw_done(&self, ui: &mut egui::Ui) {
        ui.add_space(40.0);
        ui.vertical_centered(|ui| {
            ui.label(
                egui::RichText::new("✓")
                    .size(48.0)
                    .color(SUCCESS)
            );
            ui.add_space(8.0);
            ui.label(
                egui::RichText::new("Installation complete!")
                    .size(18.0)
                    .color(SUCCESS)
                    .strong()
            );
            ui.add_space(12.0);
            ui.label(
                egui::RichText::new("Restart your terminal to use kivm.")
                    .size(14.0)
                    .color(TEXT_SECONDARY)
            );
            ui.add_space(4.0);
            ui.label(
                egui::RichText::new(format!("Installed to: {}", self.install_path.display()))
                    .size(12.0)
                    .color(TEXT_DIM)
                    .monospace()
            );
            ui.add_space(8.0);
            ui.label(
                egui::RichText::new("Try: kivm shell")
                    .size(13.0)
                    .color(ACCENT)
                    .monospace()
            );
        });
    }

    fn draw_failed(&mut self, ui: &mut egui::Ui, msg: &str) {
        ui.add_space(40.0);
        ui.vertical_centered(|ui| {
            ui.label(
                egui::RichText::new("✗")
                    .size(48.0)
                    .color(ERROR_COLOR)
            );
            ui.add_space(8.0);
            ui.label(
                egui::RichText::new("Installation failed")
                    .size(18.0)
                    .color(ERROR_COLOR)
                    .strong()
            );
            ui.add_space(12.0);
            ui.label(
                egui::RichText::new(msg)
                    .size(12.0)
                    .color(TEXT_SECONDARY)
                    .monospace()
            );
            ui.add_space(24.0);
            let retry_btn = egui::Button::new(
                egui::RichText::new("⟳  Retry")
                    .size(15.0)
                    .color(TEXT_PRIMARY)
            )
            .fill(ACCENT)
            .rounding(6.0)
            .min_size(egui::vec2(140.0, 36.0));

            if ui.add(retry_btn).clicked() {
                self.state = InstallState::Ready;
            }
        });
    }
}

// ─── Helpers ───────────────────────────────────────────────────────────────

fn default_install_path() -> PathBuf {
    if let Some(dirs) = directories::BaseDirs::new() {
        dirs.home_dir().join(".kinetix")
    } else if cfg!(target_os = "windows") {
        PathBuf::from("C:\\Kinetix")
    } else {
        PathBuf::from("/opt/kinetix")
    }
}

fn copy_dir_recursive(src: &Path, dst: &Path) -> std::io::Result<()> {
    fs::create_dir_all(dst)?;
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());
        if src_path.is_dir() {
            // Skip .git directories
            if entry.file_name() == ".git" { continue; }
            copy_dir_recursive(&src_path, &dst_path)?;
        } else {
            fs::copy(&src_path, &dst_path)?;
        }
    }
    Ok(())
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
    let icon_path = install_path.join("assets").join("KiFile.png");
    fs::write(
        apps_dir.join("kinetix.desktop"),
        format!(
            "[Desktop Entry]\nName=Kinetix\nComment=Kinetix Language Runtime\nExec={bin}/kivm exec %f\nIcon={icon}\nTerminal=true\nType=Application\nCategories=Development;\nMimeType=text/x-kinetix;\n",
            bin = install_path.join("bin").display(),
            icon = icon_path.display()
        ),
    )?;

    // Also create a shell entry for kivm shell
    fs::write(
        apps_dir.join("kinetix-shell.desktop"),
        format!(
            "[Desktop Entry]\nName=Kinetix Shell\nComment=Kinetix Interactive Terminal\nExec={bin}/kivm shell\nIcon={icon}\nTerminal=true\nType=Application\nCategories=Development;System;\n",
            bin = install_path.join("bin").display(),
            icon = icon_path.display()
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

    // Remove stale old root-level PATH entries (e.g. .kinetix without /bin)
    let install_root = path.parent().unwrap_or(path);
    let root_str = install_root.to_str().unwrap_or("");
    let mut current_path = current_path;
    if !root_str.is_empty() && root_str != path_str && current_path.contains(root_str) {
        let entries: Vec<&str> = current_path.split(';').collect();
        let cleaned: Vec<&str> = entries.into_iter()
            .filter(|e| *e != root_str)
            .collect();
        current_path = cleaned.join(";");
        env.set_value("Path", &current_path)?;
    }

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
fn register_shell(prog_id: &str, desc: &str, exe: &str, arg: &str, icon_path: &str) -> std::io::Result<()> {
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let classes = hkcu.open_subkey_with_flags("Software\\Classes", KEY_READ | KEY_WRITE)?;
    let (prog_key, _) = classes.create_subkey(prog_id)?;
    prog_key.set_value("", &desc)?;

    // Use KiFile.png as the icon
    let (icon_key, _) = prog_key.create_subkey("DefaultIcon")?;
    icon_key.set_value("", &icon_path)?;

    let (cmd_key, _) = prog_key.create_subkey("shell\\open\\command")?;
    let cmd = if arg.is_empty() {
        format!("\"{}\" \"%1\"", exe)
    } else {
        format!("\"{}\" {} \"%1\"", exe, arg)
    };
    cmd_key.set_value("", &cmd)?;
    Ok(())
}
