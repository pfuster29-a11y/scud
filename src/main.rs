#![allow(dead_code)]

use gtk4::prelude::*;
use gtk4::{glib, Application, ApplicationWindow, Box, Button, Orientation, ScrolledWindow, TextBuffer, TextView};
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

    let button = Button::builder()
        .label("Migrar a Sid, Analizar y Actualizar")
        .build();

    let text_buffer = TextBuffer::new(None);
    text_buffer.set_text("Presiona el botón para iniciar la migración y auditoría completa del sistema...");

    let text_view = TextView::builder()
        .buffer(&text_buffer)
        .editable(false)
        .wrap_mode(gtk4::WrapMode::Word)
        .build();

    let scrolled_window = ScrolledWindow::builder()
        .child(&text_view)
        .vexpand(true)
        .build();

    vbox.append(&button);
    vbox.append(&scrolled_window);

    // Conectamos el evento del botón principal
    button.connect_clicked(glib::clone!(@weak text_buffer, @weak button => move |_| {
        button.set_sensitive(false);
        text_buffer.set_text("1. Solicitando permisos, respaldando fuentes y preparando migración a Sid...\n2. Ejecutando auditoría de paquetes a continuación...");

        let (sender, receiver) = mpsc::channel();

        // Ejecutamos el flujo completo en el hilo secundario para no congelar la UI
        std::thread::spawn(move || {
            // Paso A: Preparar migración a 'sid' (incluye respaldo automático de sources.list)
            let migrator_instance = migrator::Migrator::new("/etc/apt/sources.list");
            if let Err(e) = migrator_instance.prepare_migration("sid") {
                let _ = sender.send(Err(format!("Error en el proceso de migración: {}", e)));
                return;
            }

            // Paso B: Ejecutar la auditoría y simulación con las nuevas fuentes de Sid
            let res = runner::run_full_audit();
            let _ = sender.send(res);
        });

        // Monitoreamos el canal desde el hilo principal sin bloquear la interfaz
        glib::timeout_add_local(Duration::from_millis(100), glib::clone!(@strong text_buffer, @strong button => move || {
            match receiver.try_recv() {
                Ok(result) => {
                    match result {
                        Ok(raw_output) => {
                            let changes = apt_parser::parse_apt_output(&raw_output);
                            let mut result_text = format!("=== Auditoría y Migración Scud (Sid) ===\nSe detectaron {} cambios de paquetes.\n\n", changes.len());
                            
                            if changes.is_empty() {
                                result_text.push_str("¡El sistema está listo en Sid y al día! No hay acciones pendientes.");
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