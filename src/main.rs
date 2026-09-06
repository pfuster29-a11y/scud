#![allow(dead_code)]

use gtk4::prelude::*;
use gtk4::{glib, Application, ApplicationWindow, Box, Button, CheckButton, Orientation, ScrolledWindow, TextBuffer, TextView};
use std::sync::mpsc;
use std::time::Duration;

mod apt_parser;
mod backup;
mod migrator;
mod runner;

fn main() {
    let app = Application::builder()
        .application_id("org.scud.updater")
        .build();

    app.connect_activate(build_ui);
    app.run();
}

fn build_ui(app: &Application) {
    let vbox = Box::builder()
        .orientation(Orientation::Vertical)
        .spacing(10)
        .margin_top(10)
        .margin_bottom(10)
        .margin_start(10)
        .margin_end(10)
        .build();

    // 1. Creamos una opción (checkbox) en la UI para decidir si migramos a Sid
    let check_migrator = CheckButton::builder()
        .label("Migrar fuentes de APT a Debian Sid (Unstable)")
        .active(false) // Por defecto desactivado para que elijas a voluntad
        .build();

    let button = Button::builder()
        .label("Iniciar Auditoría del Sistema")
        .build();

    let text_buffer = TextBuffer::new(None);
    text_buffer.set_text("Selecciona las opciones deseadas y presiona el botón para comenzar...");

    let text_view = TextView::builder()
        .buffer(&text_buffer)
        .editable(false)
        .wrap_mode(gtk4::WrapMode::Word)
        .build();

    let scrolled_window = ScrolledWindow::builder()
        .child(&text_view)
        .vexpand(true)
        .build();

    // Agregamos los elementos al contenedor vertical
    vbox.append(&check_migrator);
    vbox.append(&button);
    vbox.append(&scrolled_window);

    // Conectamos el evento del botón principal
    button.connect_clicked(glib::clone!(@weak text_buffer, @weak button, @weak check_migrator => move |_| {
        button.set_sensitive(false);
        let want_migrate = check_migrator.is_active();

        let status_msg = if want_migrate {
            "1. Preparando respaldo y migrando fuentes a Debian Sid...\n2. Ejecutando auditoría de paquetes..."
        } else {
            "1. Ejecutando auditoría de paquetes sobre el sistema actual..."
        };
        text_buffer.set_text(status_msg);

        let (sender, receiver) = mpsc::channel();

        // Ejecutamos en el hilo secundario
        std::thread::spawn(move || {
            // Si el usuario marcó la opción, ejecutamos el migrador primero
            if want_migrate {
                let migrator_instance = migrator::Migrator::new("/etc/apt/sources.list");
                if let Err(e) = migrator_instance.prepare_migration("sid") {
                    let _ = sender.send(Err(format!("Error en el proceso de migración: {}", e)));
                    return;
                }
            }

            // Luego ejecutamos la auditoría estándar
            let res = runner::run_full_audit();
            let _ = sender.send(res);
        });

        // Monitoreamos el canal en el hilo principal
        glib::timeout_add_local(Duration::from_millis(100), glib::clone!(@strong text_buffer, @strong button => move || {
            match receiver.try_recv() {
                Ok(result) => {
                    match result {
                        Ok(raw_output) => {
                            let changes = apt_parser::parse_apt_output(&raw_output);
                            let mut result_text = format!("=== Auditoría Scud ===\nSe detectaron {} cambios de paquetes.\n\n", changes.len());
                            
                            if changes.is_empty() {
                                result_text.push_str("¡El sistema está 100% al día! No hay acciones pendientes.");
                            } else {
                                let mut safe_count = 0;
                                let mut critical_count = 0;

                                for change in &changes {
                                    match change.risk {
                                        apt_parser::RiskLevel::Safe => safe_count += 1,
                                        apt_parser::RiskLevel::Critical => critical_count += 1,
                                    }
                                }

                                result_text.push_str(&format!("Resumen: 🟢 {} Seguros | 🔴 {} Críticos\n\n", safe_count, critical_count));
                                result_text.push_str("Detalle de paquetes:\n----------------------------------------\n");

                                for change in changes {
                                    let icon = match change.risk {
                                        apt_parser::RiskLevel::Safe => "🟢 [SEGURO]",
                                        apt_parser::RiskLevel::Critical => "🔴 [CRÍTICO]",
                                    };
                                    result_text.push_str(&format!("{} {:?} -> {}\n", icon, change.action, change.name));
                                }
                            }
                            text_buffer.set_text(&result_text);
                        }
                        Err(e) => {
                            text_buffer.set_text(&format!("Error en el proceso:\n{}", e));
                        }
                    }
                    button.set_sensitive(true);
                    glib::ControlFlow::Break
                }
                Err(mpsc::TryRecvError::Empty) => {
                    glib::ControlFlow::Continue
                }
                Err(mpsc::TryRecvError::Disconnected) => {
                    button.set_sensitive(true);
                    glib::ControlFlow::Break
                }
            }
        }));
    }));

    let window = ApplicationWindow::builder()
        .application(app)
        .title("Scud - Gestor de Actualizaciones Seguras")
        .default_width(700)
        .default_height(550)
        .child(&vbox)
        .build();

    window.present();
}