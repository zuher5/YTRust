use crate::api::services::SB_CATEGORIES;
use crate::i18n::{AppLanguage, t, tf};
use crate::store::{self, Settings};
use crate::ui::widgets::*;
use crate::ui::*;
use gpui_kit::component::input::{Input, InputEvent, InputState};

#[derive(Clone, Copy, PartialEq)]
enum Section {
    General,
    Player,
    Premium,
    Tweaks,
    SponsorBlock,
    Content,
    Subscriptions,
    Privacy,
    Network,
    Downloads,
    Data,
    About,
}

const SECTIONS: &[(Section, &str, &str)] = &[
    (Section::General, "General", "palette"),
    (Section::Player, "Player", "circle-play"),
    (Section::Premium, "Premium features", "sparkles"),
    (Section::Tweaks, "Tweaks", "sliders-horizontal"),
    (Section::SponsorBlock, "SponsorBlock & DeArrow", "shield-check"),
    (Section::Content, "Content filters", "funnel"),
    (Section::Subscriptions, "Subscriptions", "tv"),
    (Section::Privacy, "Privacy", "lock"),
    (Section::Network, "Backend & network", "network"),
    (Section::Downloads, "Downloads", "download"),
    (Section::Data, "Import / export", "database"),
    (Section::About, "About", "info"),
];

pub struct SettingsPage {
    section: Section,
    scroll: ScrollHandle,
    inputs: Vec<(&'static str, &'static str, Entity<InputState>)>,
    keyword: Entity<InputState>,
    ytdlp_version: Option<String>,
    /// whether FFmpeg was found (None until checked)
    ffmpeg_found: Option<bool>,
    /// downloads::tools_generation() when the tools were last looked for
    tools_gen: u64,
    /// open dropdown (by id) and when the last one closed (an outside click closes it
    /// right before the button's own click would reopen it)
    open_dd: Option<&'static str>,
    dd_closed_at: std::time::Instant,
    search: Entity<InputState>,
    /// SponsorBlock contribution stats for the private ID (None while loading)
    sb_stats: Option<Result<crate::api::services::SbUserInfo, String>>,
    sb_serial: u64,
    _subs: Vec<Subscription>,
}

type Setter = fn(&mut Settings, String);

impl SettingsPage {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let s = store::settings_snapshot();
        let fields: Vec<(&'static str, String, &'static str, Setter)> = vec![
            ("region", s.region.clone(), "US", |s, v| s.region = v.to_uppercase()),
            ("language", s.language.clone(), "en", |s, v| s.language = v),
            ("sb_server", s.sponsorblock_server.clone(), "https://sponsor.ajay.app", |s, v| s.sponsorblock_server = v),
            ("ytdlp", s.ytdlp_path.clone(), "yt-dlp (from PATH)", |s, v| s.ytdlp_path = v),
            ("proxy", s.proxy.clone(), "e.g. socks5://127.0.0.1:9050 or http://host:port", |s, v| s.proxy = v),
            ("external", s.external_player.clone(), "Automatic", |s, v| s.external_player = v),
            ("download_dir", s.download_dir.clone(), "Download folder", |s, v| s.download_dir = v),
            ("captions_lang", s.captions_lang.clone(), "en", |s, v| s.captions_lang = v),
            ("sb_min", format!("{}", s.sb_min_duration), "0", |s, v| s.sb_min_duration = v.parse().unwrap_or(0.0)),
            ("sb_user_id", s.sb_user_id.clone(), "Paste your private user ID", |s, v| s.sb_user_id = v),
        ];
        let mut inputs = Vec::new();
        let mut subs = Vec::new();
        for (key, val, ph, setter) in fields {
            let st = cx.new(|cx| {
                let mut i = InputState::new(window, cx).placeholder(t(ph));
                i.set_value(val, window, cx);
                i
            });
            subs.push(cx.subscribe_in(&st, window, move |this: &mut Self, st, ev: &InputEvent, _, cx| {
                if matches!(ev, InputEvent::Blur | InputEvent::PressEnter { .. }) {
                    let v = st.read(cx).value().trim().to_string();
                    let changed = key == "sb_user_id" && v != store::settings_snapshot().sb_user_id;
                    store::update_settings(|s| setter(s, v));
                    if changed {
                        this.load_sb_stats(cx);
                    }
                    if matches!(ev, InputEvent::PressEnter { .. }) {
                        toast(cx, t("Saved"));
                    }
                }
            }));
            inputs.push((key, ph, st));
        }
        let keyword = cx.new(|cx| InputState::new(window, cx).placeholder(t("Add a keyword and press Enter")));
        subs.push(cx.subscribe_in(&keyword, window, |_, st, ev: &InputEvent, window, cx| {
            if let InputEvent::PressEnter { .. } = ev {
                let v = st.read(cx).value().trim().to_string();
                if !v.is_empty() {
                    store::update_settings(|s| {
                        if !s.blocked_keywords.contains(&v) {
                            s.blocked_keywords.push(v)
                        }
                    });
                    st.update(cx, |i, cx| i.set_value("", window, cx));
                    cx.refresh_windows();
                }
            }
        }));
        let search = cx.new(|cx| InputState::new(window, cx).placeholder(t("Search settings")));
        subs.push(cx.subscribe_in(&search, window, |_, _, ev: &InputEvent, _, cx| {
            if matches!(ev, InputEvent::Change) {
                cx.notify();
            }
        }));
        let mut page = Self {
            section: Section::General,
            scroll: ScrollHandle::new(),
            inputs,
            keyword,
            ytdlp_version: None,
            ffmpeg_found: None,
            tools_gen: crate::downloads::tools_generation(),
            open_dd: None,
            dd_closed_at: std::time::Instant::now() - std::time::Duration::from_secs(5),
            search,
            sb_stats: None,
            sb_serial: 0,
            _subs: subs,
        };
        page.load_sb_stats(cx);
        page.check_tools(cx);
        page.section = Section::General;
        page
    }

    /// Looks for yt-dlp and FFmpeg (again after either was downloaded).
    fn check_tools(&mut self, cx: &mut Context<Self>) {
        cx.spawn(async move |this, cx| {
            let v = blocking(|| {
                crate::api::streams::ytdlp_path().and_then(|p| {
                    let mut c = std::process::Command::new(p);
                    c.arg("--version");
                    crate::api::streams::hide_window(&mut c);
                    c.output().ok().map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
                })
            })
            .await;
            let ff = blocking(|| crate::downloads::ffmpeg_path().is_some()).await;
            let _ = this.update(cx, |t, cx| {
                t.ytdlp_version = v;
                t.ffmpeg_found = Some(ff);
                cx.notify();
            });
        })
        .detach();
    }

    fn load_sb_stats(&mut self, cx: &mut Context<Self>) {
        let s = store::settings_snapshot();
        self.sb_serial += 1;
        let serial = self.sb_serial;
        self.sb_stats = None;
        if s.sb_user_id.trim().is_empty() {
            cx.notify();
            return;
        }
        let (server, id) = (s.sponsorblock_server.clone(), s.sb_user_id.clone());
        cx.spawn(async move |this, cx| {
            let r = blocking(move || crate::api::services::sponsor_user_info(&server, &id)).await;
            let _ = this.update(cx, |t, cx| {
                if t.sb_serial == serial {
                    t.sb_stats = Some(r.map_err(|e| crate::ui::friendly_error(&e.to_string())));
                    cx.notify();
                }
            });
        })
        .detach();
        cx.notify();
    }

    fn input(&self, key: &str) -> Entity<InputState> {
        self.inputs.iter().find(|(k, _, _)| *k == key).map(|(_, _, e)| e.clone()).unwrap()
    }
}

/// Applies a changed setting to the running app (theme, palette, current playback).
fn after_change(id: &str, window: &mut Window, cx: &mut App) {
    if id == "app-language" {
        crate::i18n::apply_component_locale();
        window.set_window_title(&format!("{} - YTRust", t("Settings")));
    } else {
        apply_theme(Some(window), cx);
        crate::player::controller::apply_settings(cx);
    }
    crate::ui::app::refresh_all_pages(cx);
}

fn card(cx: &App) -> Div {
    let p = pal(cx);
    div().flex().flex_col().rounded_xl().bg(p.surface).px_5().py_2()
}

thread_local! {
    /// Lower-case settings search text while rendering (rows that don't match hide).
    static QUERY: std::cell::RefCell<String> = const { std::cell::RefCell::new(String::new()) };
}

const CAPTION_COLORS: &[(u32, &str)] = &[
    (0xFFFFFF, "White"),
    (0xFFFF00, "Yellow"),
    (0x00FF00, "Green"),
    (0x00FFFF, "Cyan"),
    (0x0000FF, "Blue"),
    (0xFF00FF, "Magenta"),
    (0xFF0000, "Red"),
    (0x000000, "Black"),
];
const OPACITIES: &[(u32, &str)] = &[(25, "25%"), (50, "50%"), (75, "75%"), (100, "100%")];

/// How captions will look with the current settings, over a stand-in for video.
fn caption_preview(s: &Settings) -> AnyElement {
    let rgba = |rgb: u32, pct: u32| {
        let mut c: Hsla = rgb_to(rgb);
        c.a = pct.min(100) as f32 / 100.0;
        c
    };
    let text = div()
        .px_2()
        .py_0p5()
        .font_family(crate::player::controller::caption_font(&s.caption_font))
        // the player's caption size is for a 720p frame; this box is about a third of that
        .text_size(px(s.caption_size as f32 * 0.45))
        .text_color(rgba(s.caption_color, s.caption_opacity))
        .when(s.caption_background, |d| d.bg(rgba(s.caption_bg_color, s.caption_bg_opacity)))
        .child(t("Captions look like this"));
    div()
        .w(px(320.))
        .h(px(90.))
        .flex()
        .items_end()
        .justify_center()
        .pb_3()
        .rounded_lg()
        .bg(gpui_kit::linear_gradient(
            135.,
            gpui_kit::linear_color_stop(rgb_to(0x5A7FA8), 0.),
            gpui_kit::linear_color_stop(rgb_to(0x2E4A3A), 1.),
        ))
        .child(text)
        .into_any_element()
}

fn rgb_to(c: u32) -> Hsla {
    rgb(c).into()
}

fn row(title: &str, desc: &str, control: impl IntoElement, cx: &App) -> Div {
    row_text(t(title), t(desc), control, cx)
}

fn settings_search_text(text: &str) -> String {
    text.replace(['İ', 'ı'], "i").to_lowercase()
}

fn matches_settings_search(labels: &[&str]) -> bool {
    QUERY.with(|query| {
        let query = query.borrow();
        query.is_empty()
            || labels.iter().any(|label| settings_search_text(label).contains(query.as_str()))
    })
}

fn row_text(title: &str, desc: &str, control: impl IntoElement, cx: &App) -> Div {
    let p = pal(cx);
    if !matches_settings_search(&[title, desc]) {
        return div();
    }
    div()
        .flex()
        .items_center()
        .justify_between()
        // a control too wide to fit beside the text drops below it
        .flex_wrap()
        .gap_x_6()
        .gap_y_2()
        .min_w_0()
        .py_3()
        .border_b_1()
        .border_color(p.border)
        .child(
            div()
                .flex()
                .flex_col()
                .flex_1()
                // lets a long description wrap instead of widening the page,
                // but never narrower than this
                .min_w(px(240.))
                .gap_0p5()
                .child(div().text_sm().font_weight(FontWeight::MEDIUM).text_color(p.text).child(title.to_string()))
                .when(!desc.is_empty(), |d| d.child(div().text_xs().text_color(p.text2).child(desc.to_string()))),
        )
        .child(div().flex_none().child(control))
}

/// A setting whose control is too wide to sit beside its text: the control goes
/// underneath, across the full width of the card.
fn stacked_row(title: &str, desc: &str, control: impl IntoElement, cx: &App) -> Div {
    let title = t(title);
    let desc = t(desc);
    let p = pal(cx);
    if !matches_settings_search(&[title, desc]) {
        return div();
    }
    div()
        .flex()
        .flex_col()
        .gap_2()
        .py_3()
        .border_b_1()
        .border_color(p.border)
        .child(
            div()
                .flex()
                .flex_col()
                .gap_0p5()
                .child(div().text_sm().font_weight(FontWeight::MEDIUM).text_color(p.text).child(title.to_string()))
                .when(!desc.is_empty(), |d| d.child(div().text_xs().text_color(p.text2).child(desc.to_string()))),
        )
        .child(div().w_full().min_w_0().child(control))
}

fn switch(id: &'static str, on: bool, set: fn(&mut Settings, bool), cx: &App) -> impl IntoElement {
    let p = pal(cx);
    div()
        .id(id)
        .flex_none()
        .relative()
        .w(px(40.))
        .h(px(22.))
        .rounded_full()
        .cursor_pointer()
        .bg(if on { p.accent } else { p.hover })
        .child(div().absolute().top(px(3.)).left(if on { px(21.) } else { px(3.) }).size(px(16.)).rounded_full().bg(white()))
        .on_click(move |_, window, cx| {
            store::update_settings(|s| set(s, !on));
            after_change(id, window, cx);
        })
}

/// Inline segmented control, for settings with a few (3 or fewer) options.
fn segmented<T: PartialEq + Clone + 'static>(id: &'static str, options: &[(T, &str)], cur: T, set: fn(&mut Settings, T), cx: &App) -> Div {
    let p = pal(cx);
    let mut d = div().flex().flex_none().p(px(3.)).gap(px(2.)).rounded_lg().bg(p.chip);
    for (i, (v, label)) in options.iter().enumerate() {
        let v2 = v.clone();
        let active = *v == cur;
        d = d.child(
            div()
                .id(SharedString::from(format!("{id}-{i}")))
                .px_3()
                .h(px(28.))
                .flex()
                .items_center()
                .rounded_md()
                .cursor_pointer()
                .text_sm()
                .when(active, |d| d.bg(p.accent).text_color(white()).font_weight(FontWeight::MEDIUM))
                .when(!active, |d| d.text_color(p.text).hover(move |s| s.bg(p.hover)))
                .child(t(label).to_string())
                .on_click(move |_, window, cx| {
                    let v3 = v2.clone();
                    store::update_settings(|s| set(s, v3));
                    after_change(id, window, cx);
                }),
        );
    }
    d
}

fn accent_swatches(cur: u32, cx: &App) -> Div {
    let p = pal(cx);
    let mut d = div().flex().flex_none().items_center().gap_2();
    for (c, name) in [(0xff0033u32, "Red"), (0xf0307e, "Pink"), (0xab47bc, "Purple"), (0x3ea6ff, "Blue"), (0x00bfa5, "Teal"), (0x2ba640, "Green"), (0xffa000, "Amber")] {
        let active = c == cur;
        d = d.child(
            div()
                .id(SharedString::from(format!("accent-{name}")))
                .size(px(28.))
                .rounded_full()
                .p(px(3.))
                .border_2()
                .border_color(if active { p.text } else { hsla(0., 0., 0., 0.) })
                .cursor_pointer()
                .child(div().size_full().rounded_full().bg(rgb(c)).flex().items_center().justify_center().when(active, |d| d.child(icon("check").size_3().text_color(white()))))
                .on_click(move |_, window, cx| {
                    store::update_settings(|s| s.accent = c);
                    after_change("accent", window, cx);
                    crate::ui::app_icon::apply(cx);
                }),
        );
    }
    d
}

fn text_field(st: &Entity<InputState>, w: f32, cx: &App) -> impl IntoElement {
    let p = pal(cx);
    div().w(px(w)).px_3().py_1().rounded_lg().border_1().border_color(p.border).bg(p.bg).child(Input::new(st).appearance(false))
}

impl SettingsPage {
    /// The right control for a choice: inline buttons for up to 3 options, a dropdown above that.
    fn choice<T: PartialEq + Clone + 'static>(&self, id: &'static str, options: &[(T, &str)], cur: T, set: fn(&mut Settings, T), cx: &mut Context<Self>) -> AnyElement {
        if options.len() <= 3 {
            return segmented(id, options, cur, set, cx).into_any_element();
        }
        let items: Vec<(String, String, bool, T)> = options.iter().map(|(v, l)| (t(l).to_string(), String::new(), *v == cur, v.clone())).collect();
        let label = items.iter().find(|i| i.2).map(|i| i.0.clone()).unwrap_or_default();
        self.select(id, label, 200., items, set, cx)
    }

    fn app_language(&self, current: AppLanguage, cx: &mut Context<Self>) -> AnyElement {
        let mut label = String::new();
        let items = AppLanguage::OPTIONS.iter().map(|&(language, name)| {
            let selected = language == current;
            if selected {
                label = name.to_string();
            }
            let credit = match language {
                AppLanguage::Azerbaijani => "POMI (MrPOMIDOR4EK)",
                AppLanguage::Russian => "Kushida",
                _ => "",
            };
            (name.to_string(), credit.to_string(), selected, language)
        }).collect();
        self.select("app-language", label, 280., items, set_app_language, cx)
    }

    /// A dropdown button: `items` are (label, detail, active, value).
    fn select<T: Clone + 'static>(&self, id: &'static str, label: String, width: f32, items: Vec<(String, String, bool, T)>, set: fn(&mut Settings, T), cx: &mut Context<Self>) -> AnyElement {
        let p = pal(cx);
        let open = self.open_dd == Some(id);
        let mut el = div().relative().flex_none().w(px(width)).child(
            div()
                .id(id)
                .flex()
                .items_center()
                .justify_between()
                .gap_2()
                .px_3()
                .h_9()
                .rounded_lg()
                .border_1()
                .border_color(if open { p.accent } else { p.border })
                .bg(p.bg)
                .cursor_pointer()
                .hover(move |s| s.bg(p.hover))
                .child(div().flex_1().min_w_0().truncate().text_sm().text_color(p.text).child(label))
                .child(icon(if open { "chevron-up" } else { "chevron-down" }).size_4().text_color(p.text2))
                .on_click(cx.listener(move |this, _, _, cx| {
                    let just_closed = this.dd_closed_at.elapsed() < std::time::Duration::from_millis(300);
                    this.open_dd = if this.open_dd == Some(id) || just_closed { None } else { Some(id) };
                    cx.notify();
                })),
        );
        if open {
            let mut list = div()
                .id(SharedString::from(format!("{id}-list")))
                .occlude()
                .flex()
                .flex_col()
                // short enough to fit above or below the button in any window
                .max_h(px(340f32.min(f32::from(cx.global::<crate::ui::app::UiState>().viewport.height) * 0.5 - 48.).max(120.)))
                .overflow_y_scroll()
                .py_1()
                .rounded_lg()
                .bg(p.surface2)
                .border_1()
                .border_color(p.border)
                .shadow_lg()
                .on_mouse_down_out(cx.listener(|this, _, _, cx| {
                    this.open_dd = None;
                    this.dd_closed_at = std::time::Instant::now();
                    cx.notify();
                }));
            for (i, (name, detail, active, v)) in items.into_iter().enumerate() {
                list = list.child(
                    div()
                        .id(SharedString::from(format!("{id}-opt{i}")))
                        .flex()
                        .items_center()
                        .gap_3()
                        .px_3()
                        .py_1p5()
                        .cursor_pointer()
                        .text_sm()
                        .text_color(p.text)
                        .when(active, |d| d.bg(p.hover).font_weight(FontWeight::MEDIUM))
                        .hover(move |s| s.bg(p.hover))
                        .child(div().w_4().child(if active { icon("check").size_4().text_color(p.accent).into_any_element() } else { div().into_any_element() }))
                        .child(div().flex_1().child(name))
                        .when(!detail.is_empty(), |d| d.child(div().text_xs().text_color(p.text2).child(detail)))
                        .on_click(cx.listener(move |this, _, window, cx| {
                            let v = v.clone();
                            store::update_settings(|s| set(s, v));
                            this.open_dd = None;
                            after_change(id, window, cx);
                            cx.notify();
                        })),
                );
            }
            let list = list.with_animation(
                SharedString::from(format!("{id}-in")),
                Animation::new(std::time::Duration::from_millis(140)).with_easing(ease_out_quint()),
                |d, t| d.opacity(t).mt(px((1. - t) * -6.)),
            );
            // Anchored at the button: the list opens below it, or above it when
            // the window has no room below. The 40px top padding is the button's
            // height, so in either case the list sits just beside the button.
            el = el.child(
                div().absolute().top_0().left_0().w_full().child(
                    deferred(anchored().child(div().w(px(width)).pt(px(40.)).child(nested_scroll(SharedString::from(format!("{id}-wrap")), list))))
                        .with_priority(5),
                ),
            );
        }
        el.into_any_element()
    }

    /// A dropdown listing `list` (code, name); picks are written with `set`.
    fn dropdown(&self, id: &'static str, list: &'static [(&'static str, &'static str)], cur: String, set: fn(&mut Settings, String), cx: &mut Context<Self>) -> AnyElement {
        let label = format!("{} ({cur})", t(crate::locale::name(list, &cur)));
        let items = list.iter().map(|(code, name)| (t(name).to_string(), code.to_string(), code.eq_ignore_ascii_case(&cur), code.to_string())).collect();
        self.select(id, label, 280., items, set, cx)
    }

    fn premium(&self, cx: &mut Context<Self>) -> Div {
        let s = store::settings_snapshot();
        let p = pal(cx);
        let note = QUERY.with(|q| q.borrow().is_empty()).then(|| {
            div()
                .flex()
                .items_center()
                .gap_3()
                .mb_1()
                .px_4()
                .py_3()
                .rounded_lg()
                .bg(p.accent.opacity(0.12))
                .child(icon("sparkles").size_5().text_color(p.accent))
                .child(
                    div()
                        .text_sm()
                        .text_color(p.text)
                        .child(t("These are built into Stream Tube, so they work without YouTube Premium and without signing in.")),
                )
        });
        card(cx)
            .children(note)
            .child(row("Background play", "Keep playing when the window is minimized (off: pause, resume on restore)", switch("bgp", s.background_play, |s, v| s.background_play = v, cx), cx))
            .child(row(
                "Independent tab playback",
                "Play up to three tabs at once. Restart Stream Tube to apply.",
                switch("independent-tab-playback", s.independent_tab_playback, |s, v| s.independent_tab_playback = v, cx),
                cx,
            ))
            .child(row("Most replayed graph", "Heat line above the seek bar", switch("heat", s.show_heatmap, |s, v| s.show_heatmap = v, cx), cx))
            .child(row("Jump ahead", "Button to skip to the most replayed part", switch("jump", s.jump_ahead, |s, v| s.jump_ahead = v, cx), cx))
            .child(row("Enhanced bitrate", "Picks the highest-bitrate stream YouTube offers for the chosen quality", switch("ebr", s.enhanced_bitrate, |s, v| s.enhanced_bitrate = v, cx), cx))
            .child(row("Ambient mode", "Soft glow of the video's colors around the player", switch("amb", s.ambient_mode, |s, v| s.ambient_mode = v, cx), cx))
            .child(row(
                "Annotations",
                "Notes and links on older videos, restored from the Annotations Restored archive",
                switch("ann", s.annotations, |s, v| s.annotations = v, cx),
                cx,
            ))
            .child(row("Force original audio", "Play the original language instead of auto-dubbed tracks", switch("orig", s.original_audio, |s, v| s.original_audio = v, cx), cx))
    }

    fn tweaks(&self, cx: &mut Context<Self>) -> Div {
        let s = store::settings_snapshot();
        card(cx)
            .child(row("Shorts: play next automatically", "Move to the next Short when one ends", switch("sha", s.shorts_autoadvance, |s, v| s.shorts_autoadvance = v, cx), cx))
            .child(row(
                "Preload upcoming Shorts",
                "Buffer the start of this many videos ahead of playback",
                self.choice(
                    "shorts-preload",
                    &[(0, "Off"), (1, "1"), (2, "2"), (3, "3"), (4, "4"), (5, "5")],
                    s.shorts_preload.min(crate::player::shorts_preload::MAX_AHEAD),
                    |s, v| s.shorts_preload = v,
                    cx,
                ),
                cx,
            ))
            .child(row("Open Shorts in the regular player", "", switch("shp", s.shorts_in_player, |s, v| s.shorts_in_player = v, cx), cx))
            .child(row(
                "YouTube-style Shorts layout",
                "Place channel info outside the video (like YouTube) with customizable controls",
                switch("shyt", s.shorts_yt_layout, |s, v| s.shorts_yt_layout = v, cx),
                cx,
            ))
            .when(s.shorts_yt_layout, |d| {
                d.child(row(
                    "Edge navigation buttons",
                    "Place the up/down Short buttons on the far right edge",
                    switch("shen", s.shorts_edge_nav, |s, v| s.shorts_edge_nav = v, cx),
                    cx,
                ))
                .child(row("Show Subscribe button", "", switch("shsub", s.shorts_show_subscribe, |s, v| s.shorts_show_subscribe = v, cx), cx))
                .child(row("Show Dislike button", "", switch("shdis", s.shorts_show_dislike, |s, v| s.shorts_show_dislike = v, cx), cx))
            })
            .child(row("Remember last quality", "Use the quality you picked last as the default", switch("rq", s.remember_quality, |s, v| s.remember_quality = v, cx), cx))
            .child(row("Double-click for fullscreen", "", switch("dcf", s.double_click_fullscreen, |s, v| s.double_click_fullscreen = v, cx), cx))
            .child(row("Exit fullscreen when a video ends", "", switch("efe", s.exit_fullscreen_at_end, |s, v| s.exit_fullscreen_at_end = v, cx), cx))
            .child(row(
                "Controls shade",
                "Gradient behind the player controls",
                self.choice("shade", &[("light".to_string(), "Subtle"), ("default".to_string(), "Default"), ("strong".to_string(), "Strong")], s.controls_shade.clone(), |s, v| s.controls_shade = v, cx),
                cx,
            ))
    }

    fn general(&self, cx: &mut Context<Self>) -> Div {
        let s = store::settings_snapshot();
        let account = crate::api::auth::account().filter(|_| crate::api::auth::signed_in());
        let account_ctl = if account.is_some() {
            pill("acct-out", Some("log-out"), "Sign out", cx).on_click(|_, _, cx| crate::ui::app::sign_out(cx))
        } else {
            pill("acct-in", Some("circle-user"), "Sign in", cx).on_click(|_, window, cx| crate::ui::app::open_sign_in(window, cx))
        };
        let account_desc = account.map(|a| a.name).unwrap_or_else(|| t("Personal recommendations, subscriptions, likes and history").into());
        card(cx)
            .child(row_text(t("YouTube account"), &account_desc, account_ctl, cx))
            .child({
                let (desc, ctl) = match crate::api::web::account_name().filter(|_| crate::api::web::connected()) {
                    Some(name) => (
                        tf!("Connected as {0}. Videos you watch here show in your YouTube history and resume where you stopped on your other devices", name),
                        pill("web-out", Some("unlink"), "Disconnect", cx).on_click(|_, _, cx| crate::ui::app::disconnect_web(cx)),
                    ),
                    None if crate::api::auth::signed_in() => (
                        "Your YouTube sign-in already does this: videos you watch here show in your YouTube history and resume on your phone. A browser session is a fallback for when that stops working".to_string(),
                        pill("web-in", Some("history"), "Connect", cx).on_click(|_, _, cx| crate::ui::app::open_web_sign_in(cx)),
                    ),
                    None => (
                        "Sign in through your browser so videos you watch here show in your YouTube history and resume on your phone. It also plays age-restricted videos and ones YouTube shows a bot check for".to_string(),
                        pill("web-in", Some("history"), "Connect", cx).on_click(|_, _, cx| crate::ui::app::open_web_sign_in(cx)),
                    ),
                };
                row("YouTube watch history", &desc, ctl, cx)
            })
            .child(row(
                "App language",
                "",
                self.app_language(s.app_language, cx),
                cx,
            ))
            .child(row("Theme", "", self.choice("theme", &[("dark".to_string(), "Dark"), ("light".to_string(), "Light"), ("system".to_string(), "System")], s.theme.clone(), |s, v| s.theme = v, cx), cx))
            .child(row("Accent color", "Switches, highlights, progress and the seek bar", accent_swatches(s.accent, cx), cx))
            .child(row("Start page", "", self.choice("start", &[("home".to_string(), "Home"), ("subscriptions".to_string(), "Subscriptions"), ("history".to_string(), "History")], s.start_page.clone(), |s, v| s.start_page = v, cx), cx))
            .child(row(
                "Cast over LAN",
                crate::lan_cast::url(cx).unwrap_or("Send a YouTube link from your phone"),
                if let Some(url) = crate::lan_cast::url(cx).map(str::to_owned) {
                    div().flex().gap_2()
                        .child(pill("cast-copy", Some("copy"), "Copy link", cx).on_click(move |_, _, cx| copy_to_clipboard(url.clone(), cx)))
                        .child(pill("cast-stop", None, "Stop", cx).on_click(|_, _, cx| crate::lan_cast::stop(cx)))
                } else {
                    div().child(pill("cast-start", Some("play"), "Start", cx).on_click(|event, window, cx| {
                        let launch = |address, window: &mut Window, cx: &mut App| {
                            if crate::lan_cast::start(address, window.window_handle(), cx).is_err() {
                                toast(cx, t("Couldn't connect to the local network"));
                            }
                        };
                        let addresses = crate::lan_cast::addresses().unwrap_or_default();
                        match addresses.as_slice() {
                            [] => toast(cx, t("Couldn't connect to the local network")),
                            [address] => launch(*address, window, cx),
                            _ => {
                                let choices = addresses.into_iter().map(|address| {
                                    crate::ui::app::MenuItem::new("network", address.to_string(), move |window, cx| launch(address, window, cx))
                                }).collect();
                                crate::ui::app::open_menu(event.position(), choices, window, cx);
                            }
                        }
                    }))
                },
                cx,
            ))
            .child(row("Interface size", "", self.choice("scale", &[(0.9f32, "90%"), (1.0, "100%"), (1.1, "110%"), (1.25, "125%")], s.ui_scale, |s, v| s.ui_scale = v, cx), cx))
            .child(row(
                "Always show scrollbars",
                "Otherwise they appear when the pointer is over a page. Drag one, or click its track, to scroll",
                switch("sbar", s.always_scrollbars, |s, v| s.always_scrollbars = v, cx),
                cx,
            ))
            .child(row(
                "Always show tab bar",
                "Keep the tab bar visible even when only one tab is open",
                switch("tbar", s.always_show_tab_bar, |s, v| s.always_show_tab_bar = v, cx),
                cx,
            ))
            .when(cfg!(windows), |card| card.child(row(
                "Minimize to tray",
                "Keep Stream Tube open in the Windows notification area when you minimize or close it",
                switch("minimize-to-tray", s.minimize_to_tray, |s, v| s.minimize_to_tray = v, cx),
                cx,
            )))
            .child(row("Density", "", self.choice("density", &[("compact".to_string(), "Compact"), ("default".to_string(), "Default"), ("comfortable".to_string(), "Comfortable")], s.density.clone(), |s, v| s.density = v, cx), cx))
            .child(row("Thumbnail corners", "", self.choice("radius", &[(0.0f32, "Square"), (6.0, "Small"), (12.0, "Rounded"), (18.0, "Large")], s.thumb_radius, |s, v| s.thumb_radius = v, cx), cx))
            .child(row("Up next thumbnail size", "", self.choice("up-next-thumbnail-size", store::UP_NEXT_THUMBNAIL_SIZES, s.up_next_thumbnail_width, |s, v| s.up_next_thumbnail_width = v, cx), cx))
            .child(row("Grid columns", "", self.choice("cols", &[(0u32, "Auto"), (3, "3"), (4, "4"), (5, "5"), (6, "6")], s.grid_columns, |s, v| s.grid_columns = v, cx), cx))
            .child(row("Play videos on hover", "Muted preview when the pointer rests on a thumbnail", switch("hovp", s.hover_preview, |s, v| s.hover_preview = v, cx), cx))
            .child(row("Region", "Trending, Explore and search results", self.dropdown("dd-region", crate::locale::REGIONS, s.region.clone(), |s, v| s.region = v, cx), cx))
            .child(row("YouTube language", "Titles, dates and YouTube's text", self.dropdown("dd-lang", crate::locale::LANGUAGES, s.language.clone(), |s, v| s.language = v, cx), cx))
            .child(row(
                "Original video titles",
                "Use the creator's title when you open a video",
                switch("original-video-titles", s.original_video_titles, |s, v| s.original_video_titles = v, cx),
                cx,
            ))
    }

    fn player(&self, cx: &mut Context<Self>) -> Div {
        let s = store::settings_snapshot();
        card(cx)
            .child(row("Theater mode", "Use the wider player layout when opening a video", switch("theater-mode", s.theater_mode, |s, v| s.theater_mode = v, cx), cx))
            .child(row(
                "Full-height theater mode",
                "Fit the video in the space below the top bar and tabs.",
                switch("theater-full-height", s.theater_full_height, |s, v| s.theater_full_height = v, cx),
                cx,
            ))
            .child(row(
                "Picture-in-picture above other apps",
                "",
                switch("pip-always-on-top", s.pip_always_on_top, |s, v| s.pip_always_on_top = v, cx),
                cx,
            ))
            .child(row("Show video title in player", "Display the title in the upper-left corner of the player", switch("spt", s.show_player_title, |s, v| s.show_player_title = v, cx), cx))
            .child(row(
                "Default quality",
                "Maximum resolution to start videos at",
                self.choice("q", &[(2160u32, "4K"), (1440, "1440p"), (1080, "1080p"), (720, "720p"), (480, "480p"), (360, "360p")], s.default_quality, |s, v| s.default_quality = v, cx),
                cx,
            ))
            .child(row(
                "Aspect ratio",
                "Crop the black bars of letterboxed films to fill an ultrawide screen, or zoom the picture. Z changes it while watching",
                self.choice("aspect", &crate::player::aspect::MODES.iter().map(|&(v, l)| (v.to_string(), l)).collect::<Vec<_>>(), s.aspect_mode.clone(), |s, v| s.aspect_mode = v, cx),
                cx,
            ))
            .child(row("Preferred codec", "", self.choice("codec", &[("vp9".to_string(), "VP9"), ("av1".to_string(), "AV1"), ("h264".to_string(), "H.264")], s.codec_pref.clone(), |s, v| s.codec_pref = v, cx), cx))
            .child(row("Default speed", "", self.choice("speed", &[(0.75f64, "0.75x"), (1.0, "1x"), (1.25, "1.25x"), (1.5, "1.5x"), (1.75, "1.75x"), (2.0, "2x")], s.default_speed, |s, v| s.default_speed = v, cx), cx))
            .child(row("Remember speed per channel", "", switch("rsp", s.remember_speed_per_channel, |s, v| s.remember_speed_per_channel = v, cx), cx))
            .child(row("Autoplay next video", "", switch("autoplay", s.autoplay_next, |s, v| s.autoplay_next = v, cx), cx))
            .child(row("Resume where you left off", "", switch("resume", s.resume_playback, |s, v| s.resume_playback = v, cx), cx))
            .child(row("Loop videos by default", "", switch("loop", s.loop_video, |s, v| s.loop_video = v, cx), cx))
            .child(row("Audio only mode", "", switch("ao", s.audio_only, |s, v| s.audio_only = v, cx), cx))
            .child(row("Hardware decoding", "Takes effect after restart", switch("hw", s.hwdec, |s, v| s.hwdec = v, cx), cx))
            .child(row("Captions on by default", "", switch("cc", s.captions_enabled, |s, v| s.captions_enabled = v, cx), cx))
            .child(row("Caption language", "Preferred subtitle track", self.dropdown("dd-cap", crate::locale::CAPTION_LANGUAGES, s.captions_lang.clone(), |s, v| s.captions_lang = v, cx), cx))
            .child(row("Caption size", "", self.choice("capsize", &[(30u32, "Small"), (40, "Medium"), (52, "Large"), (64, "Extra large")], s.caption_size, |s, v| s.caption_size = v, cx), cx))
            .child(row(
                "Caption font",
                "",
                self.choice("capfont", &[("sans".to_string(), "Sans-serif"), ("serif".to_string(), "Serif"), ("mono".to_string(), "Monospace"), ("casual".to_string(), "Casual")], s.caption_font.clone(), |s, v| s.caption_font = v, cx),
                cx,
            ))
            .child(row("Caption color", "", self.choice("capcolor", CAPTION_COLORS, s.caption_color, |s, v| s.caption_color = v, cx), cx))
            .child(row("Caption opacity", "", self.choice("capop", OPACITIES, s.caption_opacity, |s, v| s.caption_opacity = v, cx), cx))
            .child(row("Caption background", "", switch("capbg", s.caption_background, |s, v| s.caption_background = v, cx), cx))
            .when(s.caption_background, |d| {
                d.child(row("Background color", "", self.choice("capbgc", CAPTION_COLORS, s.caption_bg_color, |s, v| s.caption_bg_color = v, cx), cx))
                    .child(row("Background opacity", "", self.choice("capbgo", OPACITIES, s.caption_bg_opacity, |s, v| s.caption_bg_opacity = v, cx), cx))
            })
            .when(!s.caption_background, |d| {
                d.child(row(
                    "Character edge",
                    "Around the text when there's no background",
                    self.choice(
                        "capedge",
                        &[("outline-shadow".to_string(), "Outline and shadow"), ("outline".to_string(), "Outline"), ("shadow".to_string(), "Drop shadow"), ("none".to_string(), "None")],
                        s.caption_edge.clone(),
                        |s, v| s.caption_edge = v,
                        cx,
                    ),
                    cx,
                ))
            })
            .child(row("Preview", "", caption_preview(&s), cx))
            .child(row("Hide controls after", "", self.choice("hide", &[(1500u64, "1.5s"), (2500, "2.5s"), (4000, "4s"), (8000, "8s")], s.controls_hide_ms, |s, v| s.controls_hide_ms = v, cx), cx))
            .child(row("Seek bar preview thumbnails", "", switch("sbprev", s.show_storyboard, |s, v| s.show_storyboard = v, cx), cx))
            .child(row("Arrow key seek step", "", self.choice("step", &[(5.0f64, "5s"), (10.0, "10s"), (15.0, "15s"), (30.0, "30s")], s.seek_step, |s, v| s.seek_step = v, cx), cx))
            .child(row(
                "External player",
                &match crate::downloads::detect_player() {
                    Some((name, _)) => tf!("Leave empty to use {0}, which is installed. Or enter the path to another player", name),
                    None => "Leave empty to use VLC or mpv when installed, or your default player. Or enter the path to another player".to_string(),
                },
                text_field(&self.input("external"), 320., cx),
                cx,
            ))

    }

    fn sponsorblock(&self, cx: &mut Context<Self>) -> Div {
        let p = pal(cx);
        let s = store::settings_snapshot();
        let mut c = card(cx)
            .child(row("Enable SponsorBlock", "", switch("sb", s.sponsorblock, |s, v| s.sponsorblock = v, cx), cx))
            .child(row("Show skip notifications", "With an “Unskip” button", switch("sbn", s.sb_notify, |s, v| s.sb_notify = v, cx), cx))
            .child(row("Minimum segment duration (seconds)", "", text_field(&self.input("sb_min"), 100., cx), cx))
            .child(row("Server", "", text_field(&self.input("sb_server"), 320., cx), cx))
            .child(stacked_row(
                "Private user ID",
                "Paste the ID from the SponsorBlock extension or another app, or generate one. Keep it secret.",
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .w_full()
                    .child({
                        let p = pal(cx);
                        div()
                            .flex_1()
                            .min_w_0()
                            .px_3()
                            .py_1()
                            .rounded_lg()
                            .border_1()
                            .border_color(p.border)
                            .bg(p.bg)
                            .child(Input::new(&self.input("sb_user_id")).appearance(false))
                    })
                    .when(!s.sb_user_id.is_empty(), |d| {
                        let id = s.sb_user_id.clone();
                        d.child(pill("sbid-copy", Some("copy"), "Copy", cx).flex_none().on_click(move |_, _, cx| {
                            copy_to_clipboard(id.clone(), cx);
                            toast(cx, t("Private ID copied"));
                        }))
                    })
                    .child(pill("sbid-gen", Some("key-round"), "Generate", cx).flex_none().on_click(cx.listener(|this, _, window, cx| {
                        let id = crate::api::services::new_sb_user_id();
                        store::update_settings(|s| s.sb_user_id = id.clone());
                        this.input("sb_user_id").update(cx, |i, cx| i.set_value(id, window, cx));
                        toast(cx, t("New SponsorBlock ID created"));
                        this.load_sb_stats(cx);
                        after_change("sbid", window, cx);
                    }))),
                cx,
            ));
        for (cat, name, color) in SB_CATEGORIES {
            let cur = s.sb_actions.get(*cat).cloned().unwrap_or_else(|| "off".into());
            let opts: Vec<(String, &str)> = [("skip", "Auto skip"), ("ask", "Ask"), ("show", "Show in seek bar"), ("off", "Off")]
                .into_iter()
                .filter(|(v, _)| !(*cat == "poi_highlight" && (*v == "skip" || *v == "ask")))
                .map(|(v, l)| (v.to_string(), l))
                .collect();
            let items = opts.iter().map(|(v, l)| (t(l).to_string(), String::new(), *v == cur, (cat.to_string(), v.clone()))).collect();
            let label = opts.iter().find(|(v, _)| *v == cur).map(|(_, l)| t(l).to_string()).unwrap_or_default();
            let id: &'static str = Box::leak(format!("sbcat-{cat}").into_boxed_str());
            let ctl = self.select(id, label, 180., items, |s, (c, v): (String, String)| {
                s.sb_actions.insert(c, v);
            }, cx);
            c = c.child(
                row(name, "", div().flex().items_center().gap_3().child(div().size_3().rounded_full().bg(rgb(*color))).child(ctl), cx),
            );
        }
        let _ = p;
        div()
            .flex()
            .flex_col()
            .gap_6()
            .child(self.sb_stats_card(cx))
            .child(c)
            .child(
                card(cx)
                    .child(row("DeArrow titles", "", switch("dat", s.dearrow_titles, |s, v| s.dearrow_titles = v, cx), cx))
                    .child(row("DeArrow thumbnails", "", switch("dath", s.dearrow_thumbnails, |s, v| s.dearrow_thumbnails = v, cx), cx))
                    .child(row("Return YouTube Dislike", "", switch("ryd", s.ryd, |s, v| s.ryd = v, cx), cx)),
            )
    }

    /// Contribution stats for the private ID, like the extension's popup.
    fn sb_stats_card(&self, cx: &mut Context<Self>) -> AnyElement {
        let p = pal(cx);
        let s = store::settings_snapshot();
        if s.sb_user_id.trim().is_empty() || !matches_settings_search(&[t("sponsorblock stats contributions segments saved")]) {
            return div().into_any_element();
        }
        let stat = |value: String, label: &str| {
            div()
                .flex()
                .flex_col()
                .flex_1()
                .gap_1()
                .p_4()
                .rounded_lg()
                .bg(p.bg)
                .child(div().text_xl().font_weight(FontWeight::BOLD).text_color(p.text).child(value))
                .child(div().text_xs().text_color(p.text2).child(t(label).to_string()))
        };
        let body = match &self.sb_stats {
            None => div().py_3().text_sm().text_color(p.text2).child(t("Loading your stats…")).into_any_element(),
            Some(Err(e)) => div()
                .flex()
                .items_center()
                .justify_between()
                .py_3()
                .child(div().text_sm().text_color(p.text2).child(tf!("Couldn't load stats: {0}", e)))
                .child(pill("sb-retry", Some("refresh-cw"), "Retry", cx).on_click(cx.listener(|this, _, _, cx| this.load_sb_stats(cx))))
                .into_any_element(),
            Some(Ok(u)) => {
                let fmt = crate::api::services::compact_count;
                div()
                    .flex()
                    .flex_col()
                    .gap_3()
                    .py_3()
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .text_sm()
                            .text_color(p.text)
                            .child(icon("shield-check").size_4().text_color(p.accent))
                            .child(if u.user_name.is_empty() { t("Your contributions").to_string() } else { tf!("{0}, your contributions", u.user_name) })
                            .when(u.vip, |d| d.child(div().px_2().rounded_full().bg(p.accent).text_xs().text_color(white()).child("VIP"))),
                    )
                    .child(
                        div()
                            .flex()
                            .gap_3()
                            .child(stat(fmt(u.segments), "segments submitted"))
                            .child(stat(fmt(u.views), "skips by others"))
                            .child(stat(crate::api::services::format_saved_minutes(u.minutes_saved), "of people's time saved"))
                            .child(stat(format!("{:.1}", u.reputation), "reputation")),
                    )
                    .when(u.segments == 0, |d| d.child(div().text_xs().text_color(p.text2).child(t("No submissions yet for this ID. Segments you submit will count here."))))
                    .when(u.warnings > 0, |d| d.child(div().text_xs().text_color(p.text2).child(tf!("{0} active warning(s) on this account", u.warnings))))
                    .into_any_element()
            }
        };
        card(cx)
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .pt_2()
                    .child(div().text_sm().font_weight(FontWeight::SEMIBOLD).text_color(p.text).child(t("Your SponsorBlock stats")))
                    .child(icon_button("sb-refresh", "refresh-cw", cx).on_click(cx.listener(|this, _, _, cx| this.load_sb_stats(cx)))),
            )
            .child(body)
            .into_any_element()
    }

    fn content(&self, cx: &mut Context<Self>) -> Div {
        let p = pal(cx);
        let s = store::settings_snapshot();
        let mut kw = div().flex().flex_wrap().gap_2();
        for k in s.blocked_keywords.iter() {
            let k2 = k.clone();
            kw = kw.child(
                div()
                    .id(SharedString::from(format!("kw-{k}")))
                    .flex()
                    .items_center()
                    .gap_1()
                    .px_3()
                    .py_1()
                    .rounded_full()
                    .bg(p.chip)
                    .text_sm()
                    .cursor_pointer()
                    .child(k.clone())
                    .child(icon("x").size_3().text_color(p.text2))
                    .on_click(move |_, _, cx| {
                        let k3 = k2.clone();
                        store::update_settings(|s| s.blocked_keywords.retain(|x| *x != k3));
                        cx.refresh_windows();
                    }),
            );
        }
        let mut chans = div().flex().flex_col();
        for (id, name) in s.blocked_channels.iter() {
            let id2 = id.clone();
            chans = chans.child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .py_1()
                    .child(div().text_sm().child(name.clone()))
                    .child(pill(SharedString::from(format!("unb-{id}")), None, "Unblock", cx).on_click(move |_, _, cx| {
                        let i = id2.clone();
                        store::update_settings(|s| s.blocked_channels.retain(|(c, _)| *c != i));
                        cx.refresh_windows();
                    })),
            );
        }
        let mut hidden = div().flex().flex_col();
        for (id, title) in s.hidden_videos.iter() {
            let id2 = id.clone();
            hidden = hidden.child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap_2()
                    .py_1()
                    .child(div().min_w_0().flex_1().truncate().text_sm().child(title.clone()))
                    .child(pill(SharedString::from(format!("unh-{id}")), None, "Unhide", cx).on_click(move |_, _, cx| {
                        let i = id2.clone();
                        store::update_settings(|s| s.hidden_videos.retain(|(v, _)| *v != i));
                        cx.refresh_windows();
                    })),
            );
        }
        div()
            .flex()
            .flex_col()
            .gap_6()
            .child(
                card(cx)
                    .child(row("Hide Shorts", "", switch("hs", s.hide_shorts, |s, v| s.hide_shorts = v, cx), cx))
                    .child(row("Hide live streams", "", switch("hl", s.hide_live, |s, v| s.hide_live = v, cx), cx))
                    .child(row("Hide upcoming / premieres", "", switch("hu", s.hide_upcoming, |s, v| s.hide_upcoming = v, cx), cx))
                    .child(row("Hide comments", "", switch("hc", s.hide_comments, |s, v| s.hide_comments = v, cx), cx))
                    .child(row("Hide related videos", "", switch("hr", s.hide_related, |s, v| s.hide_related = v, cx), cx))
                    .child(row("Hide description", "", switch("hd", s.hide_description, |s, v| s.hide_description = v, cx), cx))
                    .child(row("Hide view counts", "", switch("hv", s.hide_view_counts, |s, v| s.hide_view_counts = v, cx), cx))
                    .child(row("Hide subscriber counts", "", switch("hsc", s.hide_subscriber_counts, |s, v| s.hide_subscriber_counts = v, cx), cx))
                    .child(row("Hide channel avatars", "", switch("ha", s.hide_channel_avatars, |s, v| s.hide_channel_avatars = v, cx), cx))
                    .child(row("Hide chapters", "", switch("hch", s.hide_chapters, |s, v| s.hide_chapters = v, cx), cx))
                    .child(row("Hide Explore / trending", "", switch("he", s.hide_explore, |s, v| s.hide_explore = v, cx), cx))
                    .child(row("Hide watched videos in feeds", "", switch("hw2", s.hide_watched_in_feed, |s, v| s.hide_watched_in_feed = v, cx), cx)),
            )
            .child(
                card(cx)
                    .child(row("Blocked keywords", "", div().border_b_1().border_color(p.border).w(px(280.)).child(Input::new(&self.keyword).appearance(false)), cx))
                    .child(div().py_3().child(kw)),
            )
            .child(
                card(cx)
                    .child(row("Blocked channels", "", div(), cx))
                    .child(div().py_2().child(if s.blocked_channels.is_empty() { div().text_xs().text_color(p.text2).child(t("None")).into_any_element() } else { chans.into_any_element() })),
            )
            .child(
                card(cx)
                    .child(row("Hidden videos", "", div(), cx))
                    .child(div().py_2().child(if s.hidden_videos.is_empty() { div().text_xs().text_color(p.text2).child(t("None")).into_any_element() } else { hidden.into_any_element() })),
            )
    }

    fn subscriptions(&self, cx: &mut Context<Self>) -> Div {
        let s = store::settings_snapshot();
        card(cx)
            .child(row(
                "Feed source",
                "",
                self.choice("fs", &[("full".to_string(), "Full"), ("rss".to_string(), "RSS")], s.feed_source.clone(), |s, v| s.feed_source = v, cx),
                cx,
            ))
            .child(row("Include Shorts in feed", "Extra request per channel", switch("fis", s.feed_include_shorts, |s, v| s.feed_include_shorts = v, cx), cx))
            .child(row("Include live streams in feed", "", switch("fil", s.feed_include_live, |s, v| s.feed_include_live = v, cx), cx))
            .child(row("Group videos by day", "", switch("fgd", s.feed_group_by_day, |s, v| s.feed_group_by_day = v, cx), cx))
    }

    fn privacy(&self, cx: &mut Context<Self>) -> Div {
        let s = store::settings_snapshot();
        card(cx)
            .child(row("Save watch history", "Kept on this computer, and in your YouTube history when connected in General", switch("he2", s.history_enabled, |s, v| s.history_enabled = v, cx), cx))
            .child(row("Save search history", "", switch("se", s.search_history_enabled, |s, v| s.search_history_enabled = v, cx), cx))
            .child(row("Clear search history", "", pill("csh", Some("trash-2"), "Clear", cx).on_click(|_, _, cx| {
                store::search_history_clear();
                toast(cx, t("Search history cleared"));
            }), cx))
            .child(row("Clear watch history", "", pill("cwh", Some("trash-2"), "Clear", cx).on_click(|_, _, cx| {
                store::history_clear();
                cx.default_global::<crate::ui::cards::Watched>().0.clear();
                toast(cx, t("Watch history cleared"));
            }), cx))
    }

    fn network(&self, cx: &mut Context<Self>) -> Div {
        let s = store::settings_snapshot();
        card(cx)
            .child(row(
                "Playback client",
                "Which YouTube app Stream Tube presents itself as to get video streams. Try another one if videos stop playing or fail to load.",
                self.choice(
                    "sb2",
                    &[
                        ("auto".to_string(), "Auto (recommended)"),
                        ("tv".to_string(), "TV (plays with your sign-in)"),
                        ("visionos".to_string(), "visionOS"),
                        ("android_vr".to_string(), "Android VR"),
                        ("ytdlp".to_string(), "yt-dlp"),
                    ],
                    if s.stream_backend == "innertube" { "visionos".to_string() } else { s.stream_backend.clone() },
                    |s, v| {
                        s.stream_backend = v;
                        crate::api::streams::clear_cache();
                    },
                    cx,
                ),
                cx,
            ))
            .when(cfg!(target_os = "linux"), |d| {
                d.child(row(
                    "Display server",
                    "On Wayland desktops, X11 runs through XWayland. Choose X11 if the window doesn't appear. Takes effect after restart",
                    self.choice(
                        "display",
                        &[("auto".to_string(), "Auto"), ("wayland".to_string(), "Wayland"), ("x11".to_string(), "X11")],
                        s.display_server.clone(),
                        |s, v| s.display_server = v,
                        cx,
                    ),
                    cx,
                ))
            })
            .child(row(
                "yt-dlp path",
                &match &self.ytdlp_version {
                    Some(v) => tf!("Detected yt-dlp {0}", v),
                    None => "yt-dlp not found — install it for downloads in all qualities and the fallback extractor".into(),
                },
                text_field(&self.input("ytdlp"), 320., cx),
                cx,
            ))
            .child(row("Use IPv4 only", "Force IPv4 for new requests. Turn this off if YouTube flags your IPv4 connection.", switch("ipv4", s.ipv4_only, |s, v| s.ipv4_only = v, cx), cx))
            .child(row("Proxy", "Restart required", text_field(&self.input("proxy"), 320., cx), cx))
            .child(row("Update yt-dlp", "", pill("ytu", Some("refresh-cw"), "Update", cx).on_click(|_, _, cx| {
                cx.spawn(async move |cx| {
                    let out = blocking(|| {
                        crate::api::streams::ytdlp_path().map(|p| {
                            let mut c = std::process::Command::new(p);
                            c.arg("-U");
                            crate::api::streams::hide_window(&mut c);
                            c.output().map(|o| String::from_utf8_lossy(&o.stdout).lines().last().unwrap_or("").to_string()).unwrap_or_else(|e| e.to_string())
                        })
                    })
                    .await;
                    let _ = cx.update(|cx| toast(cx, out.unwrap_or_else(|| "yt-dlp not found".into())));
                })
                .detach();
            }), cx))
    }

    fn downloads(&self, cx: &mut Context<Self>) -> Div {
        let s = store::settings_snapshot();
        card(cx)
            .child(row("Download folder", "", text_field(&self.input("download_dir"), 360., cx), cx))
            .child(row(
                "Default format",
                "",
                self.choice("df", &crate::downloads::FORMATS.iter().map(|(k, l)| (k.to_string(), *l)).collect::<Vec<_>>(), s.download_format.clone(), |s, v| s.download_format = v, cx),
                cx,
            ))
            .child(row(
                "yt-dlp",
                &match &self.ytdlp_version {
                    Some(v) => tf!("Installed ({0}). Downloads in every quality, with SponsorBlock chapters", v),
                    None => "Needed for downloads above 360p. Stream Tube can fetch it for you".into(),
                },
                if let Some(f) = crate::downloads::ytdlp_fetch() {
                    fetch_progress("ytdlp-prog", f, cx)
                } else if self.ytdlp_version.is_some() {
                    div().into_any_element()
                } else {
                    pill("get-ytdlp", Some("download"), &tf!("Get yt-dlp ({0})", crate::downloads::YTDLP_SIZE), cx)
                        .on_click(|_, _, cx| crate::downloads::fetch_ytdlp(cx))
                        .into_any_element()
                },
                cx,
            ))
            .child(row(
                "FFmpeg",
                match self.ffmpeg_found {
                    Some(true) => "Installed. Joins the video and audio of downloads above 360p",
                    Some(false) => "Needed to join video and audio above 360p. Stream Tube can fetch it for you",
                    None => "",
                },
                if let Some(f) = crate::downloads::ffmpeg_fetch() {
                    fetch_progress("ffmpeg-prog", f, cx)
                } else if self.ffmpeg_found == Some(false) {
                    pill("get-ffmpeg", Some("download"), &tf!("Get FFmpeg ({0})", crate::downloads::FFMPEG_SIZE), cx)
                        .on_click(|_, _, cx| crate::downloads::fetch_ffmpeg(cx))
                        .into_any_element()
                } else {
                    div().into_any_element()
                },
                cx,
            ))
    }

    fn data(&self, cx: &mut Context<Self>) -> Div {
        let p = pal(cx);
        let n_subs = store::subscriptions().len();
        let n_hist = store::progress_map().len();
        let n_pl = store::playlists().len();
        div()
            .flex()
            .flex_col()
            .gap_6()
            .child(div().text_sm().text_color(p.text2).child(tf!("{0} subscriptions, {1} history entries, {2} playlists, stored in {3}", n_subs, n_hist, n_pl, store::data_dir().display())))
            .child(
                card(cx)
                    .child(row(
                        "Import",
                        "Takeout, NewPipe, FreeTube, OPML or Stream Tube backup",
                        pill("imp", Some("upload"), "Choose file…", cx).on_click(|_, _, cx| {
                            let rx = cx.prompt_for_paths(PathPromptOptions { files: true, directories: false, multiple: true, prompt: Some("Import".into()) });
                            cx.spawn(async move |cx| {
                                if let Ok(Ok(Some(paths))) = rx.await {
                                    for path in paths {
                                        let res = blocking(move || crate::import::import_file(&path)).await;
                                        let _ = cx.update(|cx| {
                                            match res {
                                                Ok(r) => toast(cx, r.to_string()),
                                                Err(e) => toast(cx, tf!("Import failed: {0}", e)),
                                            }
                                            cx.set_global(crate::ui::cards::Watched(store::progress_map()));
                                            cx.refresh_windows();
                                        });
                                    }
                                }
                            })
                            .detach();
                        }),
                        cx,
                    ))
                    .child(row("Export full backup", "", export_button("exp-all", "streamtube-backup.json", ExportKind::Backup, cx), cx))
                    .child(row("Export subscriptions (NewPipe JSON)", "", export_button("exp-np", "subscriptions.json", ExportKind::Subs, cx), cx))
                    .child(row("Export subscriptions (YouTube CSV)", "", export_button("exp-csv", "subscriptions.csv", ExportKind::Subs, cx), cx))
                    .child(row("Export subscriptions (OPML)", "For RSS readers", export_button("exp-opml", "subscriptions.opml", ExportKind::Subs, cx), cx)),
            )
    }

    fn about(&self, cx: &mut Context<Self>) -> Div {
        let s = store::settings_snapshot();
        let mpv = crate::player::mpv::available();
        card(cx)
            .child(row("YTRust", &tf!("Version {0}", env!("CARGO_PKG_VERSION")), div(), cx))
            .child(row("Video engine", if mpv { "libmpv" } else if cfg!(windows) { "libmpv-2.dll not found" } else { "libmpv not found (install the libmpv package)" }, div(), cx))
            .child(row("yt-dlp", self.ytdlp_version.as_deref().unwrap_or("not installed"), div(), cx))
            .child({
                use crate::updater::Status;
                let status = cx.default_global::<crate::updater::Updates>().status.clone();
                let text = match &status {
                    Status::Idle => t("Pre-releases included").to_string(),
                    Status::Checking => t("Checking…").to_string(),
                    Status::UpToDate => t("You have the latest version").to_string(),
                    Status::Available(r) => tf!("{0} is available", r.name),
                    Status::Downloading(_, f) => tf!("Downloading… {0}%", format_args!("{:.0}", f * 100.0)),
                    Status::Error(e) => tf!("Could not check: {0}", e),
                };
                let btn = match &status {
                    Status::Available(_) => pill("upd-show", Some("download"), "Update", cx).on_click(|_, window, cx| crate::ui::app::open_dialog(crate::ui::app::Dialog::Update, window, cx)),
                    _ => pill("upd-check", Some("refresh-cw"), "Check for updates", cx).on_click(|_, _, cx| crate::updater::check_now(true, cx)),
                };
                row("Updates", &text, btn, cx)
            })
            .child(row("Check for updates at startup", "", switch("upd-auto", s.check_updates, |s, v| s.check_updates = v, cx), cx))
            .child(row("Releases", "Changelog and downloads on GitHub", pill("upd-page", Some("external-link"), "Open", cx).on_click(|_, _, _| {
                let _ = open::that(crate::updater::RELEASES_PAGE);
            }), cx))

    }
}

#[derive(Clone, Copy)]
enum ExportKind {
    Backup,
    Subs,
}

fn export_button(id: &'static str, name: &'static str, kind: ExportKind, cx: &App) -> Stateful<Div> {
    pill(id, Some("download"), "Export…", cx).on_click(move |_, _, cx| {
        let dir = dirs::document_dir().unwrap_or_else(std::env::temp_dir);
        let rx = cx.prompt_for_new_path(&dir, Some(name));
        cx.spawn(async move |cx| {
            if let Ok(Ok(Some(path))) = rx.await {
                let p2 = path.clone();
                let res = blocking(move || match kind {
                    ExportKind::Backup => crate::import::export_backup(&p2),
                    ExportKind::Subs => crate::import::export_subscriptions(&p2),
                })
                .await;
                let _ = cx.update(|cx| match res {
                    Ok(_) => toast(cx, tf!("Exported to {0}", path.display())),
                    Err(e) => toast(cx, tf!("Export failed: {0}", e)),
                });
            }
        })
        .detach();
    })
}

impl Render for SettingsPage {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        localize_placeholder(&self.search, "Search settings", window, cx);
        localize_placeholder(&self.keyword, "Add a keyword and press Enter", window, cx);
        for (_, source, input) in &self.inputs {
            localize_placeholder(input, source, window, cx);
        }
        let tools_gen = crate::downloads::tools_generation();
        if tools_gen != self.tools_gen {
            // a tool finished downloading: show it as installed without a reload
            self.tools_gen = tools_gen;
            self.check_tools(cx);
        }
        let p = pal(cx);
        let query = settings_search_text(self.search.read(cx).value().trim());
        QUERY.with(|q| *q.borrow_mut() = query.clone());
        let mut nav = div().flex().flex_col().gap_1().w(px(240.)).flex_none().child(
            div()
                .flex()
                .items_center()
                .gap_2()
                .mb_3()
                .px_3()
                .h_9()
                .rounded_full()
                .border_1()
                .border_color(p.border)
                .bg(p.bg)
                .child(icon("search").size_4().text_color(p.text2))
                .child(div().flex_1().child(Input::new(&self.search).appearance(false).cleanable(true))),
        );
        for (sec, name, ic) in SECTIONS {
            let active = *sec == self.section;
            let sec = *sec;
            nav = nav.child(
                div()
                    .id(SharedString::from(format!("sec-{name}")))
                    .flex()
                    .items_center()
                    .gap_3()
                    .px_3()
                    .h_10()
                    .rounded_lg()
                    .cursor_pointer()
                    .text_sm()
                    .text_color(p.text)
                    .when(active, |d| d.bg(p.chip).font_weight(FontWeight::SEMIBOLD))
                    .hover(move |s| s.bg(p.hover))
                    .child(icon(ic).size_5().text_color(if active { p.accent } else { p.text }))
                    .child(t(name).to_string())
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.section = sec;
                        cx.notify();
                    })),
            );
        }
        let render_section = |this: &Self, sec: Section, cx: &mut Context<Self>| match sec {
            Section::General => this.general(cx),
            Section::Player => this.player(cx),
            Section::Premium => this.premium(cx),
            Section::Tweaks => this.tweaks(cx),
            Section::SponsorBlock => this.sponsorblock(cx),
            Section::Content => this.content(cx),
            Section::Subscriptions => this.subscriptions(cx),
            Section::Privacy => this.privacy(cx),
            Section::Network => this.network(cx),
            Section::Downloads => this.downloads(cx),
            Section::Data => this.data(cx),
            Section::About => this.about(cx),
        };
        if !query.is_empty() {
            // search: matching rows from every section
            let mut all = div().flex().flex_col().gap_4();
            for (sec, name, _) in SECTIONS {
                all = all.child(div().text_xs().font_weight(FontWeight::SEMIBOLD).text_color(p.text2).child(t(name).to_uppercase())).child(render_section(self, *sec, cx));
            }
            QUERY.with(|q| q.borrow_mut().clear());
            return page_scroll("settings-scroll", &self.scroll).child(
                div().flex().justify_center().gap_8().px_6().pt_6().pb_24().child(nav).child(
                    div().flex().flex_col().gap_4().flex_1().max_w(px(860.)).min_w_0().child(section_title(tf!("Results for “{0}”", query), cx).text_2xl()).child(all),
                ),
            );
        }
        let content = match self.section {
            Section::General => self.general(cx),
            Section::Player => self.player(cx),
            Section::Premium => self.premium(cx),
            Section::Tweaks => self.tweaks(cx),
            Section::SponsorBlock => self.sponsorblock(cx),
            Section::Content => self.content(cx),
            Section::Subscriptions => self.subscriptions(cx),
            Section::Privacy => self.privacy(cx),
            Section::Network => self.network(cx),
            Section::Downloads => self.downloads(cx),
            Section::Data => self.data(cx),
            Section::About => self.about(cx),
        };
        let title = SECTIONS.iter().find(|s| s.0 == self.section).map(|s| t(s.1)).unwrap_or("");
        page_scroll("settings-scroll", &self.scroll).child(
            div().flex().justify_center().gap_8().px_6().pt_6().pb_24().child(nav).child(
                div()
                    .flex()
                    .flex_col()
                    .gap_4()
                    .flex_1().max_w(px(860.))
                    .min_w_0()
                    .child(section_title(title, cx).text_2xl())
                    .child(content),
            ),
        )
    }
}

/// "Downloading… 42%" with a bar, shown in place of a tool's Get button.
fn fetch_progress(id: &'static str, f: crate::downloads::ToolFetch, cx: &App) -> AnyElement {
    let p = pal(cx);
    let (label, frac) = match f {
        crate::downloads::ToolFetch::Downloading(n) => (tf!("Downloading… {0}%", n), n as f32 / 100.0),
        crate::downloads::ToolFetch::Unpacking => (t("Unpacking…").to_string(), 1.0),
    };
    div()
        .id(id)
        .flex()
        .flex_col()
        .gap_1()
        .w(px(180.))
        .child(div().text_sm().text_color(p.text2).child(label))
        .child(div().h(px(4.)).w_full().rounded_full().bg(p.hover).child(div().h_full().w(relative(frac)).rounded_full().bg(p.accent)))
        .into_any_element()
}

#[cfg(test)]
mod tests {
    use super::settings_search_text;

    #[test]
    fn settings_search_accepts_dotted_and_dotless_i() {
        assert_eq!(settings_search_text("İzleme"), "izleme");
        assert_eq!(settings_search_text("AYARI"), settings_search_text("ayarı"));
    }
}

/// Changes the app language. The YouTube and caption languages follow it while
/// they still match the previous app language, so a separate choice is kept (#122).
fn set_app_language(s: &mut Settings, language: AppLanguage) {
    let old = s.app_language.code();
    if s.language.eq_ignore_ascii_case(old) {
        s.language = language.code().to_string();
    }
    if s.captions_lang.eq_ignore_ascii_case(old) {
        s.captions_lang = language.code().to_string();
    }
    s.app_language = language;
}

#[cfg(test)]
mod app_language_tests {
    use super::{AppLanguage, Settings, set_app_language};

    #[test]
    fn youtube_and_caption_languages_follow_unless_chosen_separately() {
        let mut s = Settings::default();
        set_app_language(&mut s, AppLanguage::Russian);
        assert_eq!((s.language.as_str(), s.captions_lang.as_str()), ("ru", "ru"));

        // captions set to something else on purpose stay as they are
        s.captions_lang = "ja".into();
        set_app_language(&mut s, AppLanguage::German);
        assert_eq!((s.language.as_str(), s.captions_lang.as_str()), ("de", "ja"));
        assert_eq!(s.app_language, AppLanguage::German);
    }
}
