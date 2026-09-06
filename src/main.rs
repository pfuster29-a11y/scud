#![allow(dead_code)]

use gtk4::prelude::*;
use gtk4::{
    glib, Align, Application, ApplicationWindow, Box, Button, CheckButton, Image, Label,
    ListBox, ListBoxRow, MessageDialog, MessageType, Notebook, Orientation,
    ProgressBar, ScrolledWindow,
};
use std::cell::RefCell;
use std::process::Command;
use std::rc::Rc;
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

        let list_box_clone = list_box.clone();
        let status_label_clone = status_label.clone();
        let btn_refresh_clone = btn_refresh.clone();

        glib::timeout_add_local(Duration::from_millis(100), move || -> glib::ControlFlow {
            match receiver.try_recv() {
                Ok(result) => {
                    match result {
                        Ok(raw_output) => {
                            let changes = apt_parser::parse_apt_output(&raw_output);
                            if changes.is_empty() {
                                status_label_clone.set_text("El sistema está 100% al día.");
                            } else {
                                let mut safe_count = 0;
                                let mut critical_count = 0;

                                for change in &changes {
                                    let is_safe = matches!(change.risk, apt_parser::RiskLevel::Safe);
                                    if is_safe { safe_count += 1; } else { critical_count += 1; }
                                    
                                    let desc = format!("{:?} -> Detectado en la cola de APT", change.action);
                                    list_box_clone.append(&create_package_row(&change.name, &desc, is_safe));
                                }
                                status_label_clone.set_text(&format!("{} seguras, {} problemáticas", safe_count, critical_count));
                            }
                        }
                        Err(e) => { status_label_clone.set_text(&format!("Error: {}", e)); }
                    }
                    btn_refresh_clone.set_sensitive(true);
                    glib::ControlFlow::Break
                }
                Err(mpsc::TryRecvError::Empty) => glib::ControlFlow::Continue,
                Err(mpsc::TryRecvError::Disconnected) => {
                    btn_refresh_clone.set_sensitive(true);
                    glib::ControlFlow::Break
                }
            }
        });
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

    // --- Función para mostrar ventana de progreso de actualización ---
    let window_clone_for_upgrade = window.clone();
    let run_upgrade_window = move || {
        let upgrade_win = ApplicationWindow::builder()
            .transient_for(&window_clone_for_upgrade)
            .modal(true)
            .title("Scud - Actualizando a Debian Sid")
            .default_width(500)
            .default_height(220)
            .build();

        let vbox = Box::builder()
            .orientation(Orientation::Vertical)
            .spacing(15)
            .margin_top(25)
            .margin_bottom(25)
            .margin_start(25)
            .margin_end(25)
            .build();

        let title_label = Label::builder()
            .label("Actualizando el sistema operativo...")
            .css_classes(vec!["heading".to_string()])
            .build();

        let progress_bar = ProgressBar::builder()
            .show_text(true)
            .text("Preparando actualización...")
            .build();
        progress_bar.set_pulse_step(0.05);

        let info_label = Label::builder()
            .label("Ejecutando `apt update` y `apt full-upgrade`...")
            .wrap(true)
            .justify(gtk4::Justification::Center)
            .build();

        vbox.append(&title_label);
        vbox.append(&progress_bar);
        vbox.append(&info_label);
        upgrade_win.set_child(Some(&vbox));
        upgrade_win.show();

        // Animar la barra de progreso
        let pb_clone = progress_bar.clone();
        glib::timeout_add_local(Duration::from_millis(50), move || {
            pb_clone.pulse();
            glib::ControlFlow::Continue
        });

        // Ejecutar apt update y full-upgrade en hilo independiente
        let (sender, receiver) = mpsc::channel();
        std::thread::spawn(move || {
            let update_status = Command::new("pkexec")
                .arg("apt-get")
                .arg("update")
                .status();

            match update_status {
                Ok(s) if s.success() => {
                    let _ = sender.send("Ejecutando apt full-upgrade...".to_string());
                    let upgrade_status = Command::new("pkexec")
                        .arg("apt-get")
                        .arg("full-upgrade")
                        .arg("-y")
                        .status();

                    match upgrade_status {
                        Ok(us) if us.success() => {
                            let _ = sender.send("SUCCESS".to_string());
                        }
                        Ok(_) => {
                            let _ = sender.send("ERROR: El comando full-upgrade falló.".to_string());
                        }
                        Err(e) => {
                            let _ = sender.send(format!("ERROR de ejecución: {}", e));
                        }
                    }
                }
                Ok(_) => {
                    let _ = sender.send("ERROR: El comando apt update falló.".to_string());
                }
                Err(e) => {
                    let _ = sender.send(format!("ERROR al invocar pkexec: {}", e));
                }
            }
        });

        let win_to_close = upgrade_win.clone();
        let parent_window = window_clone_for_upgrade.clone();

        glib::timeout_add_local(Duration::from_millis(500), move || {
            match receiver.try_recv() {
                Ok(msg) => {
                    win_to_close.close();
                    
                    let dialog = MessageDialog::builder()
                        .transient_for(&parent_window)
                        .modal(true)
                        .build();

                    if msg == "SUCCESS" {
                        dialog.set_message_type(MessageType::Info);
                        dialog.set_text(Some("🎉 ¡Sistema actualizado a Debian Sid con éxito!"));
                        dialog.set_secondary_text(Some(
                            "Se han aplicado todos los cambios del repositorio inestable.\n\n\
                            Es necesario reiniciar el equipo ahora para cargar el nuevo kernel y servicios."
                        ));
                        dialog.add_button("Cancelar", gtk4::ResponseType::Cancel);
                        let reboot_btn = dialog.add_button("Reiniciar Ahora", gtk4::ResponseType::Ok);
                        reboot_btn.add_css_class("suggested-action");

                        dialog.connect_response(move |dlg, response| {
                            dlg.close();
                            if response == gtk4::ResponseType::Ok {
                                let _ = Command::new("systemctl").arg("reboot").status();
                            }
                        });
                    } else {
                        dialog.set_message_type(MessageType::Error);
                        dialog.set_text(Some("❌ Hubo un error durante la actualización"));
                        dialog.set_secondary_text(Some(&format!(
                            "{}\n\n¿Deseas restaurar el archivo `sources.list` original desde el respaldo para volver a un estado seguro?",
                            msg
                        )));
                        dialog.add_button("Cerrar", gtk4::ResponseType::Close);
                        let restore_btn = dialog.add_button("Restaurar Respaldo", gtk4::ResponseType::Accept);
                        restore_btn.add_css_class("suggested-action");

                        dialog.connect_response(move |dlg, response| {
                            dlg.close();
                            if response == gtk4::ResponseType::Accept {
                                // Ejecutar la restauración y actualizar lista de paquetes
                                let cp_status = Command::new("pkexec")
                                    .arg("cp")
                                    .arg("/etc/apt/sources.list.scud.bak")
                                    .arg("/etc/apt/sources.list")
                                    .status();

                                if let Ok(s) = cp_status {
                                    if s.success() {
                                        let _ = Command::new("pkexec")
                                            .arg("apt-get")
                                            .arg("update")
                                            .status();
                                    }
                                }
                            }
                        });
                    }

                    dialog.show();
                    glib::ControlFlow::Break
                }
                Err(mpsc::TryRecvError::Empty) => glib::ControlFlow::Continue,
                Err(mpsc::TryRecvError::Disconnected) => {
                    win_to_close.close();
                    glib::ControlFlow::Break
                }
            }
        });
    };

    // --- Lógica: Migrar a Sid envuelto en Rc<RefCell<Option<...>>> ---
    let run_migration = Rc::new(RefCell::new(Some(glib::clone!(@weak tab2_status, @strong btn_migrate, @strong run_upgrade_window => move || {
        btn_migrate.set_sensitive(false);
        tab2_status.set_text("Generando respaldo y modificando fuentes a Sid...");

        let (sender, receiver) = mpsc::channel();
        std::thread::spawn(move || {
            let migrator_instance = migrator::Migrator::new("/etc/apt/sources.list");
            let res = migrator_instance.prepare_migration("sid");
            let _ = sender.send(res);
        });

        let tab2_status_clone = tab2_status.clone();
        let btn_migrate_clone = btn_migrate.clone();

        glib::timeout_add_local(Duration::from_millis(100), move || -> glib::ControlFlow {
            match receiver.try_recv() {
                Ok(result) => {
                    match result {
                        Ok(_) => {
                            tab2_status_clone.set_text("✅ ¡Fuentes modificadas a Sid! Iniciando actualización...");
                            run_upgrade_window();
                        }
                        Err(e) => {
                            tab2_status_clone.set_text(&format!("❌ Error: {}", e));
                            btn_migrate_clone.set_sensitive(true);
                        }
                    }
                    glib::ControlFlow::Break
                }
                Err(mpsc::TryRecvError::Empty) => glib::ControlFlow::Continue,
                Err(mpsc::TryRecvError::Disconnected) => {
                    btn_migrate_clone.set_sensitive(true);
                    glib::ControlFlow::Break
                }
            }
        });
    }))));

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
                Se ejecutará automáticamente un `apt full-upgrade` al finalizar la configuración de las fuentes."
            )
            .build();

        dialog.add_button("Cancelar", gtk4::ResponseType::Cancel);
        
        let accept_btn = dialog.add_button("Sí, acepto los riesgos (5s)", gtk4::ResponseType::Ok)
            .downcast::<gtk4::Button>()
            .expect("El botón de aceptación debe ser un gtk4::Button");
            
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
                if let Some(f) = run_migration.borrow_mut().take() {
                    f();
                }
            }
        }));

        dialog.show();
    }));

    window.present();
}