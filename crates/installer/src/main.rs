use eframe::egui;
use std::path::{Path, PathBuf};
use std::fs;

#[cfg(target_os = "windows")]
use winreg::enums::*;
#[cfg(target_os = "windows")]
use winreg::RegKey;

// Embed release binaries directly into the installer
const CLI_BYTES: &[u8] = include_bytes!("../../../target/release/kivm.exe");
const KICOMP_BYTES: &[u8] = include_bytes!("../../../target/release/kicomp.exe");

fn main() -> eframe::Result<()> {
    std::panic::set_hook(Box::new(|info| {
        let msg = match info.payload().downcast_ref::<&str>() {
            Some(s) => *s,
            None => match info.payload().downcast_ref::<String>() {
                Some(s) => &s[..],
                None => "Box<Any>",
            },
        };
        let location = info.location().map(|l| format!("{}:{}:{}", l.file(), l.line(), l.column())).unwrap_or_else(|| "unknown".to_string());
        let log = format!("Panic at {}: {}\n", location, msg);
        let _ = std::fs::write("crash_log.txt", log);
    }));

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([600.0, 500.0]),
        ..Default::default()
    };
    
    match eframe::run_native(
        "Kinetix Installer",
        options,
        Box::new(|cc| Ok(Box::new(InstallerApp::new(cc)))),
    ) {
        Ok(_) => Ok(()),
        Err(e) => {
            let _ = std::fs::write("crash_log.txt", format!("App error: {}", e));
            Err(e)
        }
    }
}

struct InstallerApp {
    install_path: PathBuf,
    install_kinetix: bool,
    install_kivm: bool,
    install_kicomp: bool,
    add_to_path: bool,
    status_message: String,
    is_installing: bool,
}

impl InstallerApp {
    fn new(_cc: &eframe::CreationContext<'_>) -> Self {
        let install_path = if let Some(dirs) = directories::BaseDirs::new() {
            dirs.home_dir().join(".kinetix")
        } else {
            PathBuf::from("C:\\Kinetix")
        };

        Self {
            install_path,
            install_kinetix: true,
            install_kivm: true,
            install_kicomp: true,
            add_to_path: true,
            status_message: "Ready to install.".to_string(),
            is_installing: false,
        }
    }

    fn install(&mut self) {
        self.is_installing = true;
        self.status_message = "Starting installation...".to_string();

        if let Err(e) = self.perform_install() {
             self.status_message = format!("Error: {}", e);
        } else {
             self.status_message = format!("Successfully installed to {}\n\nYou may need to restart your terminal.", self.install_path.display());
        }
        
        self.is_installing = false;
    }

    fn perform_install(&mut self) -> std::io::Result<()> {
        fs::create_dir_all(&self.install_path)?;

        let cli_path = self.install_path.join("kivm.exe");
        let comp_path = self.install_path.join("kicomp.exe");

        if self.install_kinetix || self.install_kivm {
            fs::write(&cli_path, CLI_BYTES)?;
        }

        if self.install_kicomp {
            fs::write(&comp_path, KICOMP_BYTES)?;
        }

        #[cfg(target_os = "windows")]
        {
            if self.add_to_path {
                add_to_user_path(&self.install_path)?;
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

        Ok(())
    }
}

impl eframe::App for InstallerApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading("Kinetix Installer");
            ui.add_space(20.0);

            ui.group(|ui| {
                ui.label("Components:");
                ui.checkbox(&mut self.install_kinetix, "Kinetix (Language & StdLib)");
                ui.checkbox(&mut self.install_kivm, "KiVM (Interpreter & CLI)");
                ui.checkbox(&mut self.install_kicomp, "KiComp (Build System)");
            });

            ui.add_space(10.0);

            ui.group(|ui| {
                ui.label("System Integration:");
                ui.checkbox(&mut self.add_to_path, "Add to user PATH");
            });

            ui.add_space(20.0);

            ui.horizontal(|ui| {
                ui.label("Install Location:");
                ui.label(self.install_path.to_string_lossy());
                if ui.button("Select...").clicked() {
                    if let Some(path) = rfd::FileDialog::new().pick_folder() {
                        self.install_path = path;
                    }
                }
            });

            ui.add_space(20.0);

            if ui.add_enabled(!self.is_installing, egui::Button::new("INSTALL")).clicked() {
                self.install();
            }

            ui.add_space(20.0);
            ui.separator();
            ui.label(&self.status_message);
        });
    }
}

#[cfg(target_os = "windows")]
fn add_to_user_path(path: &Path) -> std::io::Result<()> {
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let env = hkcu.open_subkey_with_flags("Environment", KEY_READ | KEY_WRITE)?;
    let current_path: String = env.get_value("Path")?;
    
    let path_str = path.to_str().ok_or(std::io::Error::new(std::io::ErrorKind::InvalidData, "Invalid path"))?;

    if !current_path.contains(path_str) {
        let new_path = if current_path.ends_with(';') {
            format!("{}{}", current_path, path_str)
        } else {
            format!("{};{}", current_path, path_str)
        };
        env.set_value("Path", &new_path)?;
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
