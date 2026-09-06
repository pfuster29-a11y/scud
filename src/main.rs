#![allow(dead_code)]

use gtk4::prelude::*;
use gtk4::{
    glib, Align, Application, ApplicationWindow, Box, Button, CheckButton, Image, Label,
    ListBox, ListBoxRow, MessageDialog, MessageType, Notebook, Orientation,
    ScrolledWindow,
};
use std::process::Command;
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

fn check_backup_tools() -> (bool, bool) {
    let timeshift = Command::new("which").arg("timeshift").status().map(|s| s.success()).unwrap_or(false);
    let snapper = Command::new("which").arg("snapper").status().map(|s| s.success()).unwrap_or(false);
    (timeshift, snapper)
}

fn create_package_row(name: &str, desc: &str, is_safe: bool) -> ListBoxRow {
    let row_box = Box::builder()
        .orientation(Orientation::Horizontal)
        .spacing(12)
        .margin_top(8)
        .margin_bottom(8)
        .margin_start(8)
        .margin_end(8)
        .build();

    let check = CheckButton::builder().valign(Align::Center).build();
    if is_safe { check.set_active(true); }

    let icon_name = if is_safe { "emblem-default" } else { "dialog-error" };
    let icon = Image::builder()
        .icon_name(icon_name)
        .icon_size(gtk4::IconSize::Large)
        .valign(Align::Center)
        .build();

    let text_box = Box::builder().orientation(Orientation::Vertical).spacing(2).build();
    
    let title = Label::builder()
        .label(name)
        .halign(Align::Start)
        .css_classes(vec!["heading".to_string()])
        .build();
        
    let subtitle = Label::builder()
        .label(desc)
        .halign(Align::Start)
        .build();

    text_box.append(&title);
    text_box.append(&subtitle);
    row_box.append(&check);
    row_box.append(&icon);
    row_box.append(&text_box);

    let row = ListBoxRow::new();
    row.set_child(Some(&row_box));
    row.set_selectable(false);
    row
}

fn build_ui(app: &Application) {
    let notebook = Notebook::new();

    // ==========================================
    // PESTAÑA 1: MANTENIMIENTO SID
    // ==========================================
    let tab1_vbox = Box::builder().orientation(Orientation::Vertical).spacing(10).margin_top(10).margin_bottom(10).margin_start(10).margin_end(10).build();

    let list_box = ListBox::new();
    list_box.set_selection_mode(gtk4::SelectionMode::None);
    list_box.add_css_class("boxed-list");

    list_box.append(&create_package_row("Scud", "Presiona 'Refrescar Lista' para auditar paquetes.", true));

    let scrolled_window = ScrolledWindow::builder()
        .child(&list_box)
        .vexpand(true)
        .build();

    let bottom_bar = Box::builder().orientation(Orientation::Horizontal).spacing(10).build();
    
    let status_label = Label::builder()
        .label("Esperando acción...")
        .hexpand(true)
        .halign(Align::Start)
        .build();

    let btn_refresh = Button::builder().label("Refrescar Lista").build();
    let btn_hold = Button::builder().label("Gestionar Retenciones").build();
    let btn_apply = Button::builder().label("Aplicar Actualizaciones Seguras").css_classes(vec!["suggested-action".to_string()]).build();

    bottom_bar.append(&status_label);
    bottom_bar.append(&btn_refresh);
    bottom_bar.append(&btn_hold);
    bottom_bar.append(&btn_apply);

    tab1_vbox.append(&scrolled_window);
    tab1_vbox.append(&bottom_bar);

    let tab1_label = Label::new(Some("Mantenimiento Sid"));
    notebook.append_page(&tab1_vbox, Some(&tab1_label));

    // --- Lógica: Refrescar Lista (Auditoría) ---
    btn_refresh.connect_clicked(glib::clone!(@weak list_box, @weak status_label, @strong btn_refresh => move |_| {
        btn_refresh.set_sensitive(false);
        status_label.set_text("Ejecutando auditoría de paquetes...");

        while let Some(child) = list_box.first_child() {
            list_box.remove(&child);
        }

        let (sender, receiver) = mpsc::channel();
        std::thread::spawn(move || {
            let res = runner::run_full_audit();
            let _ = sender.send(res);
        });

        glib::timeout_add_local(Duration::from_millis(100), glib::clone!(@weak list_box, @weak status_label, @strong btn_refresh => move || {
            match receiver.try_recv() {
                Ok(result) => {
                    match result {
                        Ok(raw_output) => {
                            let changes = apt_parser::parse_apt_output(&raw_output);
                            if changes.is_empty() {
                                status_label.set_text("El sistema está 100% al día.");
                            } else {
                                let mut safe_count = 0;
                                let mut critical_count = 0;

                                for change in &changes {
                                    let is_safe = matches!(change.risk, apt_parser::RiskLevel::Safe);
                                    if is_safe { safe_count += 1; } else { critical_count += 1; }
                                    
                                    let desc = format!("{:?} -> Detectado en la cola de APT", change.action);
                                    list_box.append(&create_package_row(&change.name, &desc, is_safe));
                                }
                                status_label.set_text(&format!("{} seguras, {} problemáticas", safe_count, critical_count));
                            }
                        }
                        Err(e) => { status_label.set_text(&format!("Error: {}", e)); }
                    }
                    btn_refresh.set_sensitive(true);
                    glib::ControlFlow::Break
                }
                Err(mpsc::TryRecvError::Empty) => glib::ControlFlow::Continue,
                Err(mpsc::TryRecvError::Disconnected) => {
                    btn_refresh.set_sensitive(true);
                    glib::ControlFlow::Break
                }
            }
        }));
    }));

    // ==========================================
    // PESTAÑA 2: MIGRAR SISTEMA
    // ==========================================
    let tab2_vbox = Box::builder().orientation(Orientation::Vertical).spacing(15).margin_top(20).margin_bottom(20).margin_start(20).margin_end(20).build();
    
    let migrate_info = Label::builder()
        .label("Utiliza esta sección para convertir tu instalación actual a Debian Sid.\nSe realizará un respaldo de /etc/apt/sources.list antes de proceder.")
        .justify(gtk4::Justification::Center)
        .build();

    let tab2_status = Label::builder().label("").halign(Align::Center).build();
    let btn_migrate = Button::builder().label("Convertir a Debian Sid (¡Riesgoso!)").halign(Align::Center).build();

    tab2_vbox.append(&migrate_info);
    tab2_vbox.append(&btn_migrate);
    tab2_vbox.append(&tab2_status);

    let tab2_label = Label::new(Some("Migrar Sistema"));
    notebook.append_page(&tab2_vbox, Some(&tab2_label));

    let window = ApplicationWindow::builder()
        .application(app)
        .title("Scud - Gestor de Actualizaciones Seguras")
        .default_width(750)
        .default_height(550)
        .child(&notebook)
        .build();

    // --- Lógica: Migrar a Sid ---
    let run_migration = glib::clone!(@weak tab2_status, @strong btn_migrate => move || {
        btn_migrate.set_sensitive(false);
        tab2_status.set_text("Generando respaldo y modificando fuentes a Sid...");

        let (sender, receiver) = mpsc::channel();
        std::thread::spawn(move || {
            let migrator_instance = migrator::Migrator::new("/etc/apt/sources.list");
            let res = migrator_instance.prepare_migration("sid");
            let _ = sender.send(res);
        });

        glib::timeout_add_local(Duration::from_millis(100), glib::clone!(@weak tab2_status, @strong btn_migrate => move || {
            match receiver.try_recv() {
                Ok(result) => {
                    match result {
                        Ok(_) => {
                            tab2_status.set_text("✅ ¡Migración completada!\nVe a 'Mantenimiento Sid' y presiona 'Refrescar Lista'.");
                        }
                        Err(e) => {
                            tab2_status.set_text(&format!("❌ Error: {}", e));
                            btn_migrate.set_sensitive(true);
                        }
                    }
                    glib::ControlFlow::Break
                }
                Err(mpsc::TryRecvError::Empty) => glib::ControlFlow::Continue,
                Err(mpsc::TryRecvError::Disconnected) => {
                    btn_migrate.set_sensitive(true);
                    glib::ControlFlow::Break
                }
            }
        }));
    });

    btn_migrate.connect_clicked(glib::clone!(@strong window, @strong run_migration => move |_| {
        let dialog = MessageDialog::builder()
            .transient_for(&window)
            .modal(true)
            .message_type(MessageType::Warning)
            .text("Advertencia Crítica: Transición a Rama Inestable (Sid)")
            .secondary_text(
                "Estás a punto de migrar el sistema operativo hacia Debian Sid (Unstable).\n\n\
                Debian Sid es un entorno de desarrollo continuo compuesto por software experimental y sin ciclos de prueba de estabilidad prolongados. Esto puede generar conflictos severos de dependencias, roturas en el gestor de arranque o pérdida de operatividad del sistema.\n\n\
                Cuidado, ¡Sid puede romper tus juguetes!\n\n\
                Se recomienda encarecidamente contar con respaldos externos antes de proceder."
            )
            .build();

        dialog.add_button("Cancelar", gtk4::ResponseType::Cancel);
        let accept_btn = dialog.add_button("Sí, acepto los riesgos (5s)", gtk4::ResponseType::Ok);
        accept_btn.set_sensitive(false);

        let countdown = std::rc::Rc::new(std::cell::Cell::new(5));
        let accept_btn_clone = accept_btn.clone();

        glib::timeout_add_seconds_local(1, move || {
            let current = countdown.get();
            if current > 1 {
                let next = current - 1;
                countdown.set(next);
                accept_btn_clone.set_label(&format!("Sí, acepto los riesgos ({}s)", next));
                glib::ControlFlow::Continue
            } else {
                accept_btn_clone.set_label("Sí, acepto los riesgos");
                accept_btn_clone.set_sensitive(true);
                glib::ControlFlow::Break
            }
        });

        dialog.connect_response(glib::clone!(@strong run_migration => move |dialog, response| {
            dialog.close();
            if response == gtk4::ResponseType::Ok {
                run_migration();
            }
        }));

        dialog.show();
    }));

    window.present();
}