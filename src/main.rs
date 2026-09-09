#![allow(dead_code)]

use gtk4::prelude::*;
use gtk4::{
    glib, Align, Application, ApplicationWindow, Box, Button, CheckButton, Image, Label,
    ListBox, ListBoxRow, MessageDialog, MessageType, Notebook, Orientation,
    ProgressBar, ScrolledWindow, TextView, WrapMode,
};
use std::cell::RefCell;
use std::process::Command;
use std::rc::Rc;
use std::sync::mpsc;
use std::time::Duration;

mod apt_parser;
mod backup;
mod migrator;
mod risk_analyzer;
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

/// Detecta si el sistema ya está corriendo Debian Sid (Unstable).
/// `/etc/debian_version` en Sid siempre contiene la palabra "sid" en su
/// contenido (ej: "trixie/sid"), independientemente de qué versión de
/// Debian sea "testing" en este momento. Es la forma más simple y
/// confiable de chequear esto sin depender de `lsb_release` (que no
/// siempre viene instalado de fábrica).
fn is_system_on_sid() -> bool {
    std::fs::read_to_string("/etc/debian_version")
        .map(|content| content.to_lowercase().contains("sid"))
        .unwrap_or(false)
}

/// Construye una fila de la lista de paquetes. El casillero arranca tildado
/// para Verdes y Amarillos (Scud avisa, pero no te impide instalar algo de
/// riesgo moderado si vos querés). Los Rojos SIEMPRE arrancan destildados —
/// esa es la única barrera que Scud no te deja saltear con un solo click;
/// si de verdad querés instalar algo rojo, tenés que tildarlo vos a mano.
fn create_package_row(name: &str, desc: &str, safety: risk_analyzer::SafetyLevel, reason: Option<&str>) -> (ListBoxRow, CheckButton) {
    use risk_analyzer::SafetyLevel;

    let row_box = Box::builder()
        .orientation(Orientation::Horizontal)
        .spacing(12)
        .margin_top(8)
        .margin_bottom(8)
        .margin_start(8)
        .margin_end(8)
        .build();

    let check = CheckButton::builder().valign(Align::Center).build();
    if safety != SafetyLevel::Red {
        check.set_active(true);
    }

    let (icon_name, level_text) = match safety {
        SafetyLevel::Green => ("emblem-default", "Seguro"),
        SafetyLevel::Yellow => ("dialog-warning", "Riesgo moderado"),
        SafetyLevel::Red => ("dialog-error", "Peligroso"),
    };
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

    // El motivo, cuando existe, se muestra siempre — es lo que le permite al
    // usuario auditar por qué Scud clasificó así este paquete en particular,
    // en vez de tener que confiar a ciegas en el color del semáforo.
    let subtitle_text = match reason {
        Some(r) => format!("{} — {} ({})", desc, level_text, r),
        None => format!("{} — {}", desc, level_text),
    };
    let subtitle = Label::builder()
        .label(&subtitle_text)
        .halign(Align::Start)
        .wrap(true)
        .build();

    text_box.append(&title);
    text_box.append(&subtitle);
    row_box.append(&check);
    row_box.append(&icon);
    row_box.append(&text_box);

    let row = ListBoxRow::new();
    row.set_child(Some(&row_box));
    row.set_selectable(false);
    (row, check)
}

fn build_ui(app: &Application) {
    let notebook = Notebook::new();

    // Creamos la ventana principal ACÁ (temprano), antes de armar las pestañas,
    // porque el manejador de "Refrescar Lista" (más abajo) necesita poder usarla
    // como ventana "padre" para el aviso de paquetes peligrosos. Todavía no le
    // asignamos contenido (`set_child`) — eso se hace más adelante, una vez que
    // el notebook ya tiene las dos pestañas completas.
    let window = ApplicationWindow::builder()
        .application(app)
        .title("Scud - Gestor de Actualizaciones Seguras")
        .default_width(750)
        .default_height(550)
        .build();

    // ==========================================
    // PESTAÑA 1: MANTENIMIENTO SID
    // ==========================================
    let tab1_vbox = Box::builder().orientation(Orientation::Vertical).spacing(10).margin_top(10).margin_bottom(10).margin_start(10).margin_end(10).build();

    let list_box = ListBox::new();
    list_box.set_selection_mode(gtk4::SelectionMode::None);
    list_box.add_css_class("boxed-list");

    let (initial_row, _initial_check) = create_package_row(
        "Scud",
        "Presiona 'Refrescar Lista' para auditar paquetes.",
        risk_analyzer::SafetyLevel::Green,
        None,
    );
    list_box.append(&initial_row);

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
    let btn_select_safe = Button::builder().label("Seleccionar Todo (excepto peligrosos)").build();
    let btn_apply = Button::builder().label("Aplicar Actualizaciones Seguras").css_classes(vec!["suggested-action".to_string()]).build();
    // Arranca deshabilitado: recién hay algo para aplicar después de un "Refrescar Lista" real.
    btn_apply.set_sensitive(false);
    btn_select_safe.set_sensitive(false);

    // Guarda, para cada paquete listado, su nombre + versiones + el casillero (checkbox)
    // correspondiente. Así, cuando el usuario toque "Aplicar Actualizaciones Seguras",
    // podemos leer exactamente cuáles quedaron tildados y cuáles no, y además guardar
    // un historial con datos reales de versión (para el sistema de "fallos previos"
    // del analizador de riesgo).
    let package_checkboxes: Rc<RefCell<Vec<(String, CheckButton, String, String, risk_analyzer::SafetyLevel)>>> = Rc::new(RefCell::new(Vec::new()));

    bottom_bar.append(&status_label);
    bottom_bar.append(&btn_refresh);
    bottom_bar.append(&btn_hold);
    bottom_bar.append(&btn_select_safe);
    bottom_bar.append(&btn_apply);

    tab1_vbox.append(&scrolled_window);
    tab1_vbox.append(&bottom_bar);

    let tab1_label = Label::new(Some("Mantenimiento Sid"));
    notebook.append_page(&tab1_vbox, Some(&tab1_label));

    // --- Lógica: Refrescar Lista (Auditoría) ---
    btn_refresh.connect_clicked(glib::clone!(@weak window, @weak list_box, @weak status_label, @strong btn_refresh, @strong btn_apply, @strong btn_select_safe, @strong package_checkboxes => move |_| {
        btn_refresh.set_sensitive(false);
        btn_apply.set_sensitive(false);
        btn_select_safe.set_sensitive(false);
        status_label.set_text("Ejecutando auditoría de paquetes...");

        while let Some(child) = list_box.first_child() {
            list_box.remove(&child);
        }
        package_checkboxes.borrow_mut().clear();

        let (sender, receiver) = mpsc::channel();
        std::thread::spawn(move || {
            let res = runner::run_full_audit();
            let _ = sender.send(res);
        });

        let list_box_clone = list_box.clone();
        let status_label_clone = status_label.clone();
        let btn_refresh_clone = btn_refresh.clone();
        let btn_apply_clone = btn_apply.clone();
        let btn_select_safe_clone = btn_select_safe.clone();
        let package_checkboxes_clone = package_checkboxes.clone();
        let window_clone = window.clone();

        glib::timeout_add_local(Duration::from_millis(100), move || -> glib::ControlFlow {
            match receiver.try_recv() {
                Ok(result) => {
                    match result {
                        Ok(raw_output) => {
                            // Cargamos el analizador de riesgo (y su historial de
                            // actualizaciones previas, si existe) recién acá, justo antes
                            // de clasificar — así cada refresco usa los datos más recientes.
                            let mut analyzer = risk_analyzer::PackageRiskAnalyzer::new();
                            let _ = analyzer.load_history();

                            let changes = apt_parser::parse_apt_output(&raw_output, &analyzer);
                            if changes.is_empty() {
                                status_label_clone.set_text("El sistema está 100% al día.");
                            } else {
                                let (green, yellow, red) = apt_parser::count_by_safety(&changes);
                                let mut red_names: Vec<String> = Vec::new();

                                for change in &changes {
                                    let desc = format!("{:?} -> Detectado en la cola de APT", change.action);
                                    let (row, checkbox) = create_package_row(
                                        &change.name,
                                        &desc,
                                        change.safety_level,
                                        change.risk_reason.as_deref(),
                                    );
                                    list_box_clone.append(&row);
                                    package_checkboxes_clone.borrow_mut().push((
                                        change.name.clone(),
                                        checkbox,
                                        change.version_from.clone(),
                                        change.version_to.clone(),
                                        change.safety_level,
                                    ));
                                    if change.safety_level == risk_analyzer::SafetyLevel::Red {
                                        red_names.push(change.name.clone());
                                    }
                                }
                                status_label_clone.set_text(&format!(
                                    "{} seguros, {} riesgo moderado, {} peligrosos",
                                    green, yellow, red
                                ));
                                btn_apply_clone.set_sensitive(true);
                                btn_select_safe_clone.set_sensitive(true);

                                // Aviso puntual (una sola vez por refresco, no invasivo) si
                                // apareció algún paquete peligroso — así no depende de que el
                                // usuario note el color en la lista.
                                if !red_names.is_empty() {
                                    let dialog = MessageDialog::builder()
                                        .transient_for(&window_clone)
                                        .modal(true)
                                        .message_type(MessageType::Warning)
                                        .text(&format!(
                                            "Se detectaron {} paquete(s) peligroso(s)",
                                            red_names.len()
                                        ))
                                        .secondary_text(&format!(
                                            "Estos paquetes quedaron destildados por defecto:\n\n{}\n\n\
                                            Revisá el motivo en cada fila de la lista antes de decidir si los querés instalar.",
                                            red_names.join("\n")
                                        ))
                                        .build();
                                    dialog.add_button("Entendido", gtk4::ResponseType::Close);
                                    dialog.connect_response(|dlg, _| dlg.close());
                                    dialog.show();
                                }
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

    // --- Lógica: "Seleccionar Todo (excepto peligrosos)" ---
    // Tilda de una todos los casilleros de Verde y Amarillo, respetando la única
    // barrera que Scud no te deja saltear con un solo click: los paquetes Rojos
    // se quedan como estén (si querés instalar uno rojo, lo tildás vos a mano,
    // fila por fila — a propósito, para que sea una decisión consciente).
    btn_select_safe.connect_clicked(glib::clone!(@strong package_checkboxes => move |_| {
        for (_, checkbox, _, _, safety) in package_checkboxes.borrow().iter() {
            if *safety != risk_analyzer::SafetyLevel::Red {
                checkbox.set_active(true);
            }
        }
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

    // Si el sistema ya está en Sid, no tiene sentido ofrecer "migrar" de nuevo:
    // deshabilitamos el botón y avisamos desde el arranque.
    if is_system_on_sid() {
        btn_migrate.set_sensitive(false);
        tab2_status.set_text("✅ Este sistema ya está corriendo Debian Sid (Unstable).");
    }

    tab2_vbox.append(&migrate_info);
    tab2_vbox.append(&btn_migrate);
    tab2_vbox.append(&tab2_status);

    let tab2_label = Label::new(Some("Migrar Sistema"));
    notebook.append_page(&tab2_vbox, Some(&tab2_label));

    // El notebook ya tiene las dos pestañas completas: recién ahora le asignamos
    // el contenido a la ventana (creada más arriba, antes de la pestaña 1).
    window.set_child(Some(&notebook));

    // --- Función para mostrar ventana de progreso de actualización ---
    let window_clone_for_upgrade = window.clone();
    let tab2_status_for_upgrade = tab2_status.clone();
    let btn_migrate_for_upgrade = btn_migrate.clone();
    let run_upgrade_window = move || {
        let upgrade_win = ApplicationWindow::builder()
            .transient_for(&window_clone_for_upgrade)
            .modal(true)
            .title("Scud - Actualizando a Debian Sid")
            .default_width(600)
            .default_height(420)
            .build();

        let vbox = Box::builder()
            .orientation(Orientation::Vertical)
            .spacing(10)
            .margin_top(20)
            .margin_bottom(20)
            .margin_start(20)
            .margin_end(20)
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

        // --- Terminal en vivo: muestra el output real de apt línea por línea ---
        let terminal_view = TextView::builder()
            .editable(false)
            .cursor_visible(false)
            .monospace(true)
            .wrap_mode(WrapMode::WordChar)
            .build();
        terminal_view.add_css_class("scud-terminal");

        let terminal_scroll = ScrolledWindow::builder()
            .child(&terminal_view)
            .vexpand(true)
            .min_content_height(220)
            .build();

        vbox.append(&title_label);
        vbox.append(&progress_bar);
        vbox.append(&terminal_scroll);
        upgrade_win.set_child(Some(&vbox));
        upgrade_win.show();

        // Animar la barra de progreso (pulso indeterminado mientras corre apt).
        // Envuelto en RefCell porque SourceId no es Copy y necesitamos poder
        // sacarlo (una sola vez) desde dentro de un closure FnMut más abajo.
        let pb_clone = progress_bar.clone();
        let pulse_id = glib::timeout_add_local(Duration::from_millis(50), move || {
            pb_clone.pulse();
            glib::ControlFlow::Continue
        });
        let pulse_source = Rc::new(RefCell::new(Some(pulse_id)));

        // Helper para agregar una línea a la terminal y hacer autoscroll.
        let append_line = {
            let terminal_view = terminal_view.clone();
            move |text: &str| {
                let buffer = terminal_view.buffer();
                let mut end_iter = buffer.end_iter();
                buffer.insert(&mut end_iter, text);
                buffer.insert(&mut end_iter, "\n");
                // Autoscroll al final
                let end_mark = buffer.create_mark(None, &buffer.end_iter(), false);
                terminal_view.scroll_mark_onscreen(&end_mark);
            }
        };

        // Ejecutar apt update y luego full-upgrade en un hilo independiente,
        // transmitiendo el output real en vivo (línea por línea) a través del
        // canal en lugar de esperar en silencio a que termine todo el proceso.
        let (sender, receiver) = mpsc::channel::<runner::AptLine>();
        std::thread::spawn(move || {
            // `run_privileged_apt_streaming` es síncrona: corre el comando entero
            // y recién vuelve cuando terminó. La usamos primero con un canal
            // "sonda" para el update, así podemos decidir si seguimos con el
            // full-upgrade sin cerrar el canal principal antes de tiempo.
            let (update_tx, update_rx) = mpsc::channel::<runner::AptLine>();
            runner::run_privileged_apt_streaming(&["update"], update_tx);

            let mut update_result: Result<(), String> =
                Err("El proceso de 'apt update' no devolvió resultado.".to_string());

            for msg in update_rx {
                match msg {
                    runner::AptLine::Output(line) => {
                        let _ = sender.send(runner::AptLine::Output(line));
                    }
                    runner::AptLine::Done(res) => {
                        update_result = res;
                    }
                }
            }

            match update_result {
                Ok(()) => {
                    let _ = sender.send(runner::AptLine::Output(
                        "— apt update OK. Iniciando full-upgrade —".to_string(),
                    ));
                    runner::run_privileged_apt_streaming(&["full-upgrade", "-y"], sender);
                }
                Err(e) => {
                    let _ = sender.send(runner::AptLine::Done(Err(format!(
                        "Falló 'apt update': {}",
                        e
                    ))));
                }
            }
        });

        let win_to_close = upgrade_win.clone();
        let parent_window = window_clone_for_upgrade.clone();
        let progress_bar_clone = progress_bar.clone();
        let tab2_status_clone2 = tab2_status_for_upgrade.clone();
        let btn_migrate_clone2 = btn_migrate_for_upgrade.clone();

        // Sondeamos el canal seguido (cada 80ms) para que la terminal se sienta
        // "en vivo" en lugar de actualizarse a los tirones.
        glib::timeout_add_local(Duration::from_millis(80), move || {
            // Drenamos todos los mensajes disponibles en esta pasada, no solo uno,
            // para no quedarnos atrás si apt larga muchas líneas de golpe.
            loop {
                match receiver.try_recv() {
                    Ok(runner::AptLine::Output(line)) => {
                        append_line(&line);
                    }
                    Ok(runner::AptLine::Done(result)) => {
                        if let Some(id) = pulse_source.borrow_mut().take() {
                            id.remove();
                        }
                        progress_bar_clone.set_fraction(1.0);
                        win_to_close.close();

                        let dialog = MessageDialog::builder()
                            .transient_for(&parent_window)
                            .modal(true)
                            .build();

                        match result {
                            Ok(()) => {
                                // El sistema ya quedó en Sid (paquetes instalados), independientemente
                                // de si el usuario reinicia ahora o más tarde. Reflejamos eso en la
                                // pestaña de migración: el botón queda deshabilitado para siempre.
                                tab2_status_clone2.set_text(
                                    "✅ Migración completada. El sistema está en Debian Sid — reiniciá cuanto antes."
                                );
                                btn_migrate_clone2.set_sensitive(false);

                                dialog.set_message_type(MessageType::Info);
                                dialog.set_text(Some("🎉 ¡Sistema actualizado a Debian Sid con éxito!"));
                                dialog.set_secondary_text(Some(
                                    "Se han aplicado todos los cambios del repositorio inestable.\n\n\
                                    Es necesario reiniciar el equipo para cargar el nuevo kernel y servicios.\n\n\
                                    ⚠️ Si no reiniciás ahora, el sistema puede quedar en un estado inestable: \
                                    algunos servicios seguirían corriendo en memoria con versiones viejas de \
                                    librerías, mientras el disco ya tiene las nuevas. Te recomendamos reiniciar \
                                    cuanto antes, aunque no sea en este instante."
                                ));
                                dialog.add_button("Reiniciar Más Tarde", gtk4::ResponseType::Cancel);
                                let reboot_btn = dialog.add_button("Reiniciar Ahora", gtk4::ResponseType::Ok);
                                reboot_btn.add_css_class("suggested-action");

                                dialog.connect_response(move |dlg, response| {
                                    dlg.close();
                                    if response == gtk4::ResponseType::Ok {
                                        let _ = Command::new("systemctl").arg("reboot").status();
                                    }
                                });
                            }
                            Err(msg) => {
                                // Falló apt (no la preparación de sources.list, que ya había salido bien
                                // antes de llegar acá). Reactivamos el botón para permitir reintentar.
                                tab2_status_clone2.set_text(&format!("❌ Error durante la actualización: {}", msg));
                                btn_migrate_clone2.set_sensitive(true);

                                dialog.set_message_type(MessageType::Error);
                                dialog.set_text(Some("❌ Hubo un error durante la actualización"));
                                dialog.set_secondary_text(Some(&format!(
                                    "{}\n\nRevisá el output de la terminal para más detalle. \
                                    ¿Deseas restaurar el archivo `sources.list` original desde el respaldo para volver a un estado seguro?",
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
                        }

                        dialog.show();
                        return glib::ControlFlow::Break;
                    }
                    Err(mpsc::TryRecvError::Empty) => break,
                    Err(mpsc::TryRecvError::Disconnected) => {
                        if let Some(id) = pulse_source.borrow_mut().take() {
                            id.remove();
                        }
                        win_to_close.close();
                        return glib::ControlFlow::Break;
                    }
                }
            }
            glib::ControlFlow::Continue
        });
    };

    // --- Función para mostrar ventana de progreso de "Aplicar Actualizaciones Seguras" ---
    // Muy similar a run_upgrade_window, pero con pasos extra: retener (hold) los
    // paquetes que el usuario destildó ANTES de actualizar, liberarlos (unhold) al
    // final pase lo que pase, y guardar en el historial del analizador de riesgo
    // qué paquetes se aplicaron con éxito (para que la próxima auditoría ya sepa
    // si alguno viene con antecedentes de fallo).
    let window_clone_for_apply = window.clone();
    let status_label_for_apply = status_label.clone();
    let btn_apply_for_apply = btn_apply.clone();
    let btn_refresh_for_apply = btn_refresh.clone();
    let run_apply_window = move |held_packages: Vec<String>, applied_packages: Vec<(String, String, String)>| {
        let apply_win = ApplicationWindow::builder()
            .transient_for(&window_clone_for_apply)
            .modal(true)
            .title("Scud - Aplicando Actualizaciones Seguras")
            .default_width(600)
            .default_height(420)
            .build();

        let vbox = Box::builder()
            .orientation(Orientation::Vertical)
            .spacing(10)
            .margin_top(20)
            .margin_bottom(20)
            .margin_start(20)
            .margin_end(20)
            .build();

        let title_label = Label::builder()
            .label("Actualizando paquetes seguros...")
            .css_classes(vec!["heading".to_string()])
            .build();

        let progress_bar = ProgressBar::builder()
            .show_text(true)
            .text("Preparando actualización...")
            .build();
        progress_bar.set_pulse_step(0.05);

        let terminal_view = TextView::builder()
            .editable(false)
            .cursor_visible(false)
            .monospace(true)
            .wrap_mode(WrapMode::WordChar)
            .build();

        let terminal_scroll = ScrolledWindow::builder()
            .child(&terminal_view)
            .vexpand(true)
            .min_content_height(220)
            .build();

        vbox.append(&title_label);
        vbox.append(&progress_bar);
        vbox.append(&terminal_scroll);
        apply_win.set_child(Some(&vbox));
        apply_win.show();

        let pb_clone = progress_bar.clone();
        let pulse_id = glib::timeout_add_local(Duration::from_millis(50), move || {
            pb_clone.pulse();
            glib::ControlFlow::Continue
        });
        let pulse_source = Rc::new(RefCell::new(Some(pulse_id)));

        let append_line = {
            let terminal_view = terminal_view.clone();
            move |text: &str| {
                let buffer = terminal_view.buffer();
                let mut end_iter = buffer.end_iter();
                buffer.insert(&mut end_iter, text);
                buffer.insert(&mut end_iter, "\n");
                let end_mark = buffer.create_mark(None, &buffer.end_iter(), false);
                terminal_view.scroll_mark_onscreen(&end_mark);
            }
        };

        let (sender, receiver) = mpsc::channel::<runner::AptLine>();
        let held_packages_for_thread = held_packages.clone();
        std::thread::spawn(move || {
            let held_packages = held_packages_for_thread;

            // 1. Retener los paquetes que el usuario destildó, si hay alguno.
            if !held_packages.is_empty() {
                let _ = sender.send(runner::AptLine::Output(format!(
                    "— Reteniendo {} paquete(s) excluido(s) por el usuario: {} —",
                    held_packages.len(),
                    held_packages.join(", ")
                )));
                if let Err(e) = runner::run_apt_mark("hold", &held_packages) {
                    let _ = sender.send(runner::AptLine::Done(Err(format!(
                        "No se pudieron retener los paquetes seleccionados: {}",
                        e
                    ))));
                    return;
                }
            }

            // Libera los holds al final, pase lo que pase, y recién ahí avisa a la UI.
            let finish = |sender: &mpsc::Sender<runner::AptLine>, held: &[String], result: Result<(), String>| {
                if !held.is_empty() {
                    let _ = sender.send(runner::AptLine::Output(
                        "— Liberando la retención de los paquetes excluidos —".to_string(),
                    ));
                    let _ = runner::run_apt_mark("unhold", held);
                }
                let _ = sender.send(runner::AptLine::Done(result));
            };

            // 2. apt update
            let (update_tx, update_rx) = mpsc::channel::<runner::AptLine>();
            runner::run_privileged_apt_streaming(&["update"], update_tx);
            let mut update_result: Result<(), String> =
                Err("El proceso de 'apt update' no devolvió resultado.".to_string());
            for msg in update_rx {
                match msg {
                    runner::AptLine::Output(line) => {
                        let _ = sender.send(runner::AptLine::Output(line));
                    }
                    runner::AptLine::Done(res) => {
                        update_result = res;
                    }
                }
            }

            if let Err(e) = update_result {
                finish(&sender, &held_packages, Err(format!("Falló 'apt update': {}", e)));
                return;
            }

            let _ = sender.send(runner::AptLine::Output(
                "— apt update OK. Iniciando full-upgrade (paquetes retenidos excluidos automáticamente) —".to_string(),
            ));

            // 3. apt full-upgrade -y (los paquetes con hold quedan afuera automáticamente)
            let (fu_tx, fu_rx) = mpsc::channel::<runner::AptLine>();
            runner::run_privileged_apt_streaming(&["full-upgrade", "-y"], fu_tx);
            let mut fu_result: Result<(), String> =
                Err("El proceso de 'full-upgrade' no devolvió resultado.".to_string());
            for msg in fu_rx {
                match msg {
                    runner::AptLine::Output(line) => {
                        let _ = sender.send(runner::AptLine::Output(line));
                    }
                    runner::AptLine::Done(res) => {
                        fu_result = res;
                    }
                }
            }

            finish(&sender, &held_packages, fu_result);
        });

        let win_to_close = apply_win.clone();
        let parent_window = window_clone_for_apply.clone();
        let progress_bar_clone = progress_bar.clone();
        let status_label_clone2 = status_label_for_apply.clone();
        let btn_apply_clone2 = btn_apply_for_apply.clone();
        let btn_refresh_clone2 = btn_refresh_for_apply.clone();
        let applied_packages_clone = applied_packages.clone();

        glib::timeout_add_local(Duration::from_millis(80), move || {
            loop {
                match receiver.try_recv() {
                    Ok(runner::AptLine::Output(line)) => {
                        append_line(&line);
                    }
                    Ok(runner::AptLine::Done(result)) => {
                        if let Some(id) = pulse_source.borrow_mut().take() {
                            id.remove();
                        }
                        progress_bar_clone.set_fraction(1.0);
                        win_to_close.close();

                        btn_apply_clone2.set_sensitive(true);
                        btn_refresh_clone2.set_sensitive(true);

                        // Guardamos en el historial del analizador de riesgo qué paquetes
                        // se aplicaron con éxito. Esto es lo que le permite, en el futuro,
                        // marcar como Rojo un paquete que ya falló antes en este mismo sistema.
                        // Nota: si `apt full-upgrade` falla a mitad de camino, no tenemos forma
                        // confiable de saber cuáles de estos paquetes llegaron a instalarse y
                        // cuáles no — por honestidad, solo registramos historial en el caso de
                        // éxito total.
                        if result.is_ok() && !applied_packages_clone.is_empty() {
                            let mut analyzer = risk_analyzer::PackageRiskAnalyzer::new();
                            let _ = analyzer.load_history();
                            let now = std::time::SystemTime::now()
                                .duration_since(std::time::UNIX_EPOCH)
                                .map(|d| d.as_secs() as i64)
                                .unwrap_or(0);
                            for (name, version_from, version_to) in &applied_packages_clone {
                                analyzer.record_update(risk_analyzer::UpdateRecord {
                                    package: name.clone(),
                                    version_from: version_from.clone(),
                                    version_to: version_to.clone(),
                                    success: true,
                                    timestamp: now,
                                });
                            }
                            let _ = analyzer.save_history();
                        }

                        let dialog = MessageDialog::builder()
                            .transient_for(&parent_window)
                            .modal(true)
                            .build();

                        match result {
                            Ok(()) => {
                                status_label_clone2.set_text("✅ Actualizaciones seguras aplicadas con éxito.");
                                dialog.set_message_type(MessageType::Info);
                                dialog.set_text(Some("🎉 ¡Actualizaciones aplicadas con éxito!"));
                                dialog.set_secondary_text(Some(
                                    "Se instalaron los paquetes seguros. Los paquetes que dejaste \
                                    destildados no fueron tocados.\n\n\
                                    Te recomendamos volver a presionar \"Refrescar Lista\" para ver \
                                    el estado actualizado del sistema."
                                ));
                                dialog.add_button("Cerrar", gtk4::ResponseType::Close);
                            }
                            Err(msg) => {
                                status_label_clone2.set_text(&format!("❌ Error al aplicar actualizaciones: {}", msg));
                                dialog.set_message_type(MessageType::Error);
                                dialog.set_text(Some("❌ Hubo un error al aplicar las actualizaciones"));
                                dialog.set_secondary_text(Some(&format!(
                                    "{}\n\nRevisá el output de la terminal para más detalle.",
                                    msg
                                )));
                                dialog.add_button("Cerrar", gtk4::ResponseType::Close);
                            }
                        }

                        dialog.connect_response(|dlg, _| dlg.close());
                        dialog.show();
                        return glib::ControlFlow::Break;
                    }
                    Err(mpsc::TryRecvError::Empty) => break,
                    Err(mpsc::TryRecvError::Disconnected) => {
                        if let Some(id) = pulse_source.borrow_mut().take() {
                            id.remove();
                        }
                        btn_apply_clone2.set_sensitive(true);
                        btn_refresh_clone2.set_sensitive(true);
                        win_to_close.close();
                        return glib::ControlFlow::Break;
                    }
                }
            }
            glib::ControlFlow::Continue
        });
    };

    // --- Lógica: botón "Aplicar Actualizaciones Seguras" ---
    // Antes de arrancar, chequea si hay Snapper/Timeshift; si no hay ninguno, avisa y
    // deja continuar bajo riesgo del usuario (todavía no creamos el snapshot en sí,
    // eso queda para el próximo paso).
    btn_apply.connect_clicked(glib::clone!(
        @strong window, @strong package_checkboxes, @strong run_apply_window
        => move |_| {
        let held_packages: Vec<String> = package_checkboxes
            .borrow()
            .iter()
            .filter(|(_, checkbox, _, _, _)| !checkbox.is_active())
            .map(|(name, _, _, _, _)| name.clone())
            .collect();

        let applied_packages: Vec<(String, String, String)> = package_checkboxes
            .borrow()
            .iter()
            .filter(|(_, checkbox, _, _, _)| checkbox.is_active())
            .map(|(name, _, vf, vt, _)| (name.clone(), vf.clone(), vt.clone()))
            .collect();

        let (has_timeshift, has_snapper) = check_backup_tools();

        if !has_timeshift && !has_snapper {
            let dialog = MessageDialog::builder()
                .transient_for(&window)
                .modal(true)
                .message_type(MessageType::Warning)
                .text("No se detectó Snapper ni Timeshift")
                .secondary_text(
                    "No tenés instalada ninguna herramienta de snapshots (Snapper o Timeshift). \
                    Sin un punto de restauración, si algo sale mal durante esta actualización va \
                    a ser más difícil volver atrás.\n\n\
                    Podés continuar bajo tu propio riesgo, o cancelar e instalar alguna de las dos antes."
                )
                .build();

            dialog.add_button("Cancelar", gtk4::ResponseType::Cancel);
            let continue_btn = dialog.add_button("Continuar (Riesgo)", gtk4::ResponseType::Ok);
            continue_btn.add_css_class("destructive-action");

            let run_apply_window = run_apply_window.clone();
            dialog.connect_response(move |dlg, response| {
                dlg.close();
                if response == gtk4::ResponseType::Ok {
                    run_apply_window(held_packages.clone(), applied_packages.clone());
                }
            });
            dialog.show();
        } else {
            run_apply_window(held_packages, applied_packages);
        }
    }));


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