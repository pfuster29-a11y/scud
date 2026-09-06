#![allow(dead_code)]

use gtk4::prelude::*;
use gtk4::{
    glib, Align, Application, ApplicationWindow, Box, Button, CheckButton, Image, Label,
    ListBox, ListBoxRow, MessageDialog, ButtonsType, MessageType, Notebook, Orientation,
    ScrolledWindow,
};
use std::process::Command;

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

/// Crea una fila visual para la lista de paquetes (Check + Icono + Textos)
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

    // Filas de prueba para visualizar el diseño
    list_box.append(&create_package_row("libc6", "Seguro para actualizar: v2.36 -> v2.37", true));
    list_box.append(&create_package_row("systemd", "Actualización RIESGOSA: v252 -> v253 (Conflicto detectado)", false));

    let scrolled_window = ScrolledWindow::builder()
        .child(&list_box)
        .vexpand(true)
        .build();

    let bottom_bar = Box::builder().orientation(Orientation::Horizontal).spacing(10).build();
    
    let status_label = Label::builder()
        .label("1 actualización segura, 1 problemática")
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

    // ==========================================
    // PESTAÑA 2: MIGRAR SISTEMA
    // ==========================================
    let tab2_vbox = Box::builder().orientation(Orientation::Vertical).spacing(15).margin_top(20).margin_bottom(20).margin_start(20).margin_end(20).build();
    
    let migrate_info = Label::builder()
        .label("Utiliza esta sección para convertir tu instalación actual a Debian Sid.\nSe realizará un respaldo de /etc/apt/sources.list antes de proceder.")
        .justify(gtk4::Justification::Center)
        .build();

    let btn_migrate = Button::builder().label("Convertir a Debian Sid (¡Riesgoso!)").halign(Align::Center).build();

    tab2_vbox.append(&migrate_info);
    tab2_vbox.append(&btn_migrate);

    let tab2_label = Label::new(Some("Migrar Sistema"));
    notebook.append_page(&tab2_vbox, Some(&tab2_label));

    // Ventana Principal
    let window = ApplicationWindow::builder()
        .application(app)
        .title("Scud - Gestor de Actualizaciones Seguras")
        .default_width(750)
        .default_height(550)
        .child(&notebook)
        .build();

    // Lógica del botón de migración (Mantenemos la validación de backups)
    btn_migrate.connect_clicked(glib::clone!(@strong window => move |_| {
        let (has_timeshift, has_snapper) = check_backup_tools();
        if !has_timeshift && !has_snapper {
            let dialog = MessageDialog::builder()
                .transient_for(&window)
                .modal(true)
                .message_type(MessageType::Warning)
                .buttons(ButtonsType::OkCancel)
                .text("Aviso de Seguridad: Sin Herramientas de Respaldo")
                .secondary_text("No se detectó Timeshift ni Snapper.\n¿Deseas proceder bajo tu propio riesgo?")
                .build();

            dialog.connect_response(|dialog, response| {
                dialog.close();
                if response == gtk4::ResponseType::Ok {
                    println!("Ejecutando migración..."); // Aquí conectaremos el migrator real luego
                }
            });
            dialog.show();
        } else {
            println!("Herramientas detectadas. Ejecutando migración...");
        }
    }));

    window.present();
}