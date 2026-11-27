#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
mod api;
mod assets;
mod display;
mod downloads;
mod feed;
mod i18n;
mod idle_inhibit;
mod import;
mod lan_cast;
mod links;
mod locale;
#[cfg(windows)]
mod media_controls;
#[cfg(target_os = "linux")]
mod media_session;
#[cfg(target_os = "linux")]
mod mpris;
mod perf;
mod player;
mod recommend;
mod store;
#[cfg(windows)]
mod tray;
mod ui;
mod updater;

use gpui_kit::*;

/// Window placement from the last session (size, position, maximized), or a
/// centered default sized to the screen.
fn saved_window_bounds(cx: &App) -> WindowBounds {
    if let Some(v) =
        store::kv_get("window").and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
    {
        let f = |k: &str| v.get(k).and_then(|x| x.as_f64()).unwrap_or(0.0) as f32;
        let (w, h) = (f("w"), f("h"));
        if w >= 640.0 && h >= 400.0 {
            let b = Bounds::new(point(px(f("x")), px(f("y"))), size(px(w), px(h)));
            // ignore positions that are no longer on any display
            let visible = cx.displays().iter().any(|d| d.bounds().intersects(&b));
            if visible {
                return if v
                    .get("maximized")
                    .and_then(|m| m.as_bool())
                    .unwrap_or(false)
                {
                    WindowBounds::Maximized(b)
                } else {
                    WindowBounds::Windowed(b)
                };
            }
        }
    }
    let display = cx
        .primary_display()
        .map(|d| d.bounds().size)
        .unwrap_or(size(px(1600.), px(1000.)));
    let sz = size(
        (display.width * 0.82).min(px(1600.)),
        (display.height * 0.85).min(px(1000.)),
    );
    WindowBounds::Windowed(Bounds::centered(None, sz, cx))
}

pub fn save_window_bounds(window: &Window) {
    let (b, maximized) = match window.window_bounds() {
        WindowBounds::Windowed(b) => (b, false),
        WindowBounds::Maximized(b) => (b, true),
        WindowBounds::Fullscreen(b) => (b, false),
    };
    let v = serde_json::json!({
        "x": f32::from(b.origin.x), "y": f32::from(b.origin.y),
        "w": f32::from(b.size.width), "h": f32::from(b.size.height),
        "maximized": maximized,
    });
    store::kv_set("window", &v.to_string());
}

struct WatchHistoryFlushed;
impl Global for WatchHistoryFlushed {}

pub(crate) fn flush_watch_history(cx: &mut App) {
    if cx.try_global::<WatchHistoryFlushed>().is_some() {
        return;
    }
    cx.set_global(WatchHistoryFlushed);
    player::controller::player(cx).update(cx, |p, _| p.finish_playback_history());
    // The network worker does not depend on the UI thread. GPUI only allows
    // 200 ms for async quit handlers, so wait here with a bounded exit delay.
    if let Some(pending) = api::web::pending_reports() {
        if pending.recv_timeout(std::time::Duration::from_secs(2)).is_err() {
            eprintln!("streamtube: YouTube watch reports did not finish before exit");
        }
    }
}

fn main() {
    perf::mark("main");
    // the local stream proxy must never be reached through the user's proxy
    // SAFETY: set before any other thread starts
    unsafe {
        std::env::set_var("no_proxy", "127.0.0.1,localhost,::1");
        std::env::set_var("NO_PROXY", "127.0.0.1,localhost,::1");
    }
    perf::init();
    store::init();
    perf::mark("store");
    display::choose();
    perf::mark("display");
    // one running app: a second launch (e.g. a clicked streamtube: link) hands over and exits
    let arg = std::env::args().nth(1).unwrap_or_default();
    if !display::relaunched() && links::forward_to_running(&arg) {
        return;
    }
    let incoming = links::listen();
    perf::mark("links");
    gpui_kit::application()
        .with_assets(assets::Assets)
        .with_http_client(ui::UreqHttp::new())
        .run(|cx: &mut App| {
            perf::mark("gpui running");
            gpui_kit::init(cx);
            i18n::apply_component_locale();
            perf::mark("gpui_kit init");
            player::preview::init(cx);
            perf::mark("preview init");
            cx.set_global(ui::Pal::new(true, store::settings_snapshot().accent));
            ui::apply_theme(None, cx);
            player::controller::init(cx);
            perf::mark("player init");
            let opts = WindowOptions {
                // Without this the Wayland/X11 window has no identity, so docks and
                // taskbars can't match it to streamtube.desktop (StartupWMClass).
                app_id: Some("streamtube".into()),
                window_bounds: Some(saved_window_bounds(cx)),
                titlebar: Some(TitlebarOptions {
                    title: Some("YTRust".into()),
                    appears_transparent: true,
                    ..Default::default()
                }),
                window_min_size: Some(size(px(720.), px(480.))),
                // GPUI draws an inactive window at most ~30 times a second by default, so a
                // 60 fps video watched next to another app ran at half rate; frames are only
                // drawn when something changed, so an idle background window still costs nothing
                inactive_frame_interval: None,
                ..Default::default()
            };
            gpui_kit::open_window(opts, cx, |window, cx| {
                ui::apply_theme(Some(window), cx);
                #[cfg(windows)]
                media_controls::init(window, cx);
                #[cfg(target_os = "linux")]
                mpris::init(cx);
                let app = cx.new(|cx| ui::app::AppView::new(window, cx));
                cx.new(|_| ui::app::Shell::new(app))
            })
            .expect("failed to open window");
            perf::mark("window open");
            ui::app_icon::apply(cx);
            links::register_scheme();
            perf::mark("icon");
            if let Some(mut rx) = incoming {
                cx.spawn(async move |cx| {
                    use futures::StreamExt;
                    while let Some(arg) = rx.next().await {
                        let _ = cx.update(|cx| {
                            let pip = ui::pip::handle(cx);
                            let Some(main) = cx.windows().into_iter().find(|w| Some(*w) != pip)
                            else {
                                return;
                            };
                            let _ = main.update(cx, |_, window, cx| {
                                ui::app::restore_window(window);
                                window.activate_window();
                                if let Some(route) = ui::app::route_from_link(&arg) {
                                    ui::app::navigate(route, window, cx);
                                }
                            });
                        });
                    }
                })
                .detach();
            }
            cx.on_app_quit(|cx| {
                idle_inhibit::release(cx);
                std::future::ready(())
            })
            .detach();
            cx.on_window_closed(|cx, id| {
                ui::pip::window_closed(id, cx);
                // the picture-in-picture window alone doesn't keep the app running
                let pip = ui::pip::handle(cx);
                if cx.windows().iter().all(|w| Some(*w) == pip) {
                    for player in player::controller::tab_players(cx) {
                        player.update(cx, |p, _| p.save_progress(true));
                    }
                    flush_watch_history(cx);
                    idle_inhibit::release(cx);
                    cx.quit();
                }
            })
            .detach();
            cx.on_app_quit(|cx| {
                flush_watch_history(cx);
                async {}
            })
            .detach();
        });
}
