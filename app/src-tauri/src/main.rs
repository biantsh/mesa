#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod browse;
#[cfg(target_os = "macos")]
mod cgimage;
mod clipboard;
mod cloud;
mod clues;
mod code_scope;
mod commands;
mod crawl;
mod document_preview;
mod editor;
mod embed;
mod epub_read;
mod estimate;
mod examples;
mod folder_access;
mod folder_counts;
#[cfg(target_os = "linux")]
mod gnome_keys;
mod highlight;
mod hotkey;
mod index;
mod indexer;
mod instance;
mod iwork_read;
mod main_window;
mod matches;
mod notices;
mod page_scale;
mod power;
mod preview_queue;
mod progress;
mod reader;
mod reclaim;
mod relevance;
mod rules;
mod settings;
mod shown_folder;
mod shown_listing;
mod startup;
mod store;
mod surface;
mod survey;
mod system_menu;
mod thumbnail;
#[cfg(windows)]
mod toast;
mod unindexable;
mod update;
mod vectors;
mod watch;
#[cfg(target_os = "linux")]
mod wayland;
mod widget;
mod words;
#[cfg(any(target_os = "linux", test))]
mod xdg;

use mesa_ocr as ocr;

use tauri::{
    Manager, WindowEvent,
    menu::{Menu, MenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
};

fn add_tray_icon(app: &tauri::App, search_key: Option<&str>) -> tauri::Result<()> {
    let open = MenuItem::with_id(app, "open", "Open Mesa", true, None::<&str>)?;
    let search = MenuItem::with_id(app, "search", hotkey::tray_text(search_key), true, None::<&str>)?;
    app.manage(hotkey::TrayItem(search.clone()));
    let quit = MenuItem::with_id(app, "quit", "Quit Mesa", true, None::<&str>)?;
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    let menu = Menu::with_items(app, &[&open, &search, &quit])?;
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    let menu = {
        let status = MenuItem::with_id(app, "status", "Not indexing", false, None::<&str>)?;
        let pause = MenuItem::with_id(app, "pause", "Pause Indexing", false, None::<&str>)?;
        let line = tauri::menu::PredefinedMenuItem::separator(app)?;
        let menu = Menu::with_items(app, &[&status, &pause, &line, &open, &search, &quit])?;
        app.manage(surface::menu_bar::Items { status, pause });
        menu
    };
    let tray = TrayIconBuilder::with_id("mesa");
    let mut tray = tray
        .tooltip("Mesa")
        .menu(&menu)
        .show_menu_on_left_click(cfg!(target_os = "macos"))
        .on_menu_event(|app, event| match event.id.as_ref() {
            "pause" => {
                let index = app.state::<index::Index>();
                let paused = index.progress().is_some_and(|progress| progress.paused);
                if let Err(e) = index.set_paused(!paused) {
                    eprintln!("Mesa: {e}");
                }
            }
            "open" => main_window::show(app),
            "search" => widget::show(app),
            update::MENU_ID => update::open_dialog(app),
            "quit" => {
                app.state::<index::Index>().shut_down();
                app.exit(0);
            }
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if !cfg!(target_os = "macos")
                && let TrayIconEvent::Click {
                    button: MouseButton::Left,
                    button_state: MouseButtonState::Up,
                    ..
                } = event
            {
                main_window::show(tray.app_handle());
            }
        });
    #[cfg(target_os = "macos")]
    let template = menu_bar_icon();
    #[cfg(not(target_os = "macos"))]
    let template: Option<tauri::image::Image<'static>> = None;
    if let Some(icon) = template {
        tray = tray.icon(icon).icon_as_template(true);
    } else if let Some(icon) = app.default_window_icon() {
        tray = tray.icon(icon.clone());
    }
    tray.build(app)?;
    app.state::<update::Updates>().attach_menu(menu);
    Ok(())
}

#[cfg(target_os = "macos")]
fn menu_bar_icon() -> Option<tauri::image::Image<'static>> {
    let mut decoder = png::Decoder::new(std::io::Cursor::new(
        include_bytes!("../icons/tray-template.png").as_slice(),
    ));
    decoder.set_transformations(png::Transformations::EXPAND);
    let mut reader = decoder.read_info().ok()?;
    let mut pixels = vec![0; reader.output_buffer_size()?];
    let frame = reader.next_frame(&mut pixels).ok()?;
    if frame.color_type != png::ColorType::Rgba || frame.bit_depth != png::BitDepth::Eight {
        return None;
    }
    pixels.truncate(frame.buffer_size());
    Some(tauri::image::Image::new_owned(pixels, frame.width, frame.height))
}

fn main() {
    if reader::run_if_asked() {
        return;
    }
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    if embed::watchdog::run_if_asked() {
        return;
    }
    let context = tauri::generate_context!();
    #[cfg(target_os = "linux")]
    xdg::set_up_gstreamer(&context.config().identifier);
    let data_dir = startup::data_dir(&context.config().identifier);
    let claim = match data_dir.as_deref() {
        None => Some(instance::Claim::default()),
        Some(dir) => match instance::claim(dir) {
            instance::Launch::First(claim) => Some(claim),
            #[cfg(target_os = "macos")]
            instance::Launch::Later => {
                instance::hand_over(dir, &startup::arguments());
                return;
            }
            #[cfg(not(target_os = "macos"))]
            instance::Launch::Later => None,
        },
    };
    main_window::keep_hidden_at_start(startup::starts_without_window());
    let previews = thumbnail::PreviewServer::start();
    if claim.is_some() {
        reader::start();
    }
    let kept_previews = previews.clone();
    let managed_previews = previews.clone();

    let builder = tauri::Builder::default();
    #[cfg(not(target_os = "macos"))]
    let builder = builder.plugin(tauri_plugin_single_instance::init(|app, args, _cwd| {
        startup::another_launch(app, &args)
    }));
    builder
        .plugin(
            tauri_plugin_global_shortcut::Builder::new()
                .with_handler(|app, _shortcut, event| {
                    if event.state() == tauri_plugin_global_shortcut::ShortcutState::Pressed {
                        hotkey::pressed(app, widget::toggle);
                    }
                })
                .build(),
        )
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            Some(vec![startup::BACKGROUND_ARG]),
        ))
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_dialog::init())
        .on_window_event(|window, event| match (window.label(), event) {
            (widget::LABEL, WindowEvent::CloseRequested { api, .. }) => {
                api.prevent_close();
                widget::hide(window.app_handle());
            }
            (widget::LABEL, WindowEvent::Focused(false)) => widget::lost_focus(window.app_handle()),
            (widget::LABEL, WindowEvent::Focused(true)) => widget::got_focus(),
            (_, WindowEvent::CloseRequested { api, .. }) => {
                api.prevent_close();
                main_window::closed(window);
            }
            _ => {}
        })
        .setup(move |app| {
            #[cfg(target_os = "macos")]
            startup::stay_out_of_relaunch_at_login();
            #[cfg(target_os = "linux")]
            page_scale::watch();
            main_window::fit_to_page_scale(app);
            let Some(claim) = claim else {
                std::process::exit(0);
            };
            let dir = data_dir.ok_or("Couldn't find the folder for Mesa's data.")?;
            if update::just_updated(&dir) {
                main_window::keep_hidden_at_start(false);
            }
            let previews_dir = match std::env::var_os("MESA_DATA_DIR").filter(|dir| !dir.is_empty()) {
                Some(_) => dir.join("previews"),
                None => app.path().app_cache_dir()?.join("previews"),
            };
            kept_previews.keep_on_disk_in(previews_dir);
            let resources = app.path().resource_dir().ok();
            let settings = settings::SettingsStore::open(&dir);
            let embedder = embed::for_this_platform(&dir, resources.as_deref(), settings.get().device());
            embedder.warm_up();
            let index = index::Index::open_with(&dir, embedder, false, settings.get().indexed_before())
                .map_err(|e| format!("Couldn't open the index in {}: {e}", dir.display()))?;
            index.set_always_full_speed(settings.get().always_full_speed);
            let access = folder_access::FolderAccess::open(&dir, app.path().home_dir().ok().as_deref());
            access.indexing(index.indexed_roots().iter().map(std::path::PathBuf::as_path));
            app.manage(access);
            app.manage(estimate::Estimator::open(&dir, resources.as_deref()));
            index.check_for_changes();
            index.start_watching();
            app.manage(index);
            app.manage(shown_folder::ShownFolder::default());
            app.manage(shown_listing::ShownListing::default());
            app.manage(folder_counts::FolderCounts::default());
            surface::start(app.handle().clone(), dir.clone());
            startup::at_start(app.handle(), &settings);
            let search_key = hotkey::Hotkey::new(app.handle(), settings.get().search_hotkey.as_deref());
            app.manage(settings);
            app.manage(update::Updates::open(app.handle(), &dir));
            add_tray_icon(app, search_key.name().as_deref())?;
            app.manage(search_key);
            if startup::wants_search(&startup::arguments()) {
                widget::show_soon(app.handle());
            }
            main_window::set_up_dock(app);
            main_window::watch_full_screen(app);
            widget::float_over_everything(app.handle());
            system_menu::install(app.handle());
            main_window::show_if_the_page_does_not(app);
            update::start(app.handle());
            app.resources_table().add(claim.ready(app.handle(), &dir));
            Ok(())
        })
        .manage(managed_previews)
        .register_asynchronous_uri_scheme_protocol("thumb", move |_ctx, request, responder| {
            previews.handle(request, responder)
        })
        .invoke_handler(tauri::generate_handler![
            commands::home_dir,
            commands::list_dir,
            commands::refresh_folder,
            commands::watch_shown_folder,
            commands::show_folder_previews,
            commands::show_preview_window,
            commands::index_view,
            commands::search,
            commands::search_more,
            commands::match_details,
            commands::paging,
            commands::allow_playing,
            commands::set_indexing_paused,
            commands::set_indexing_turbo,
            commands::get_indexed_types,
            commands::set_indexed_types,
            commands::count_indexed_of_type,
            commands::count_indexed_in,
            commands::folder_file_counts,
            commands::window_buttons_middle,
            commands::add_to_index,
            commands::remove_from_index,
            commands::exclude_from_index,
            commands::stop_excluding,
            commands::count_with_data,
            commands::index_lists,
            commands::stop_indexing,
            commands::open_file,
            commands::copy_files,
            commands::open_project_page,
            commands::notices,
            commands::open_notice_link,
            commands::show_in_folder,
            commands::index_everything,
            commands::survey_user_files,
            commands::index_estimate,
            commands::open_privacy_settings,
            commands::get_settings,
            commands::get_search_hotkey,
            commands::set_search_hotkey,
            commands::begin_hotkey_recording,
            commands::end_hotkey_recording,
            commands::hotkey_keys_released,
            commands::embedding_gpu,
            commands::model_status,
            commands::retry_model_setup,
            commands::set_gpu_acceleration,
            commands::open_code,
            commands::code_file,
            commands::also_in,
            commands::code_projects,
            commands::code_peek,
            commands::set_always_full_speed,
            commands::set_start_at_sign_in,
            commands::page_painted,
            commands::reveal_main,
            commands::hide_widget,
            commands::report_page_error,
            commands::set_picking,
            commands::folders_among,
            commands::prepare_example,
            commands::example_progress,
            commands::open_main,
            commands::show_main,
            commands::set_widget_boxes,
            commands::widget_fills_screen,
            commands::update_status,
            commands::check_for_updates,
            commands::install_update,
            commands::set_check_for_updates,
        ])
        .build(context)
        .expect("error while building tauri application")
        .run(|app, event| match event {
            #[cfg(target_os = "macos")]
            tauri::RunEvent::Reopen { .. } => main_window::show(app),
            tauri::RunEvent::Exit => {
                if let Some(index) = app.try_state::<index::Index>() {
                    index.shut_down();
                }
            }
            _ => {}
        });
}
