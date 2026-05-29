use std::cell::RefCell;

use ksni::menu::{StandardItem, SubMenu};
use ksni::{Handle, MenuItem, Tray, TrayService};

use crate::ctl;

#[derive(Debug, Clone)]
pub enum TrayCmd {
    Show,
    Start,
    Stop,
    Quit,
}

pub struct TrayState {
    status_text: String,
    running: bool,
    clients: Vec<String>,
    tx: async_channel::Sender<TrayCmd>,
}

fn icon_for(_running: bool, clients: usize) -> &'static str {
    // Use the freedesktop colored status icons rather than -symbolic variants:
    // symbolic icons inherit the panel's monochrome tint, which defeats the
    // point of "green when in use". user-available renders as a green dot in
    // every mainstream icon theme; user-offline as grey.
    if clients > 0 {
        "user-available"
    } else {
        "user-offline"
    }
}

impl Tray for TrayState {
    fn id(&self) -> String {
        // Short identifier rather than the reverse-domain app ID. Some tray
        // hosts (Plasma-derived ones) surface this in tooltips/popups, and
        // "io.github.wayhelm.Wayhelm" is needlessly long there.
        "wayhelm".into()
    }

    fn title(&self) -> String {
        "Wayhelm".into()
    }

    fn icon_name(&self) -> String {
        icon_for(self.running, self.clients.len()).into()
    }

    fn tool_tip(&self) -> ksni::ToolTip {
        let mut description = self.status_text.clone();
        if !self.clients.is_empty() {
            description.push('\n');
            for c in &self.clients {
                description.push_str("\n• ");
                description.push_str(c);
            }
        }
        ksni::ToolTip {
            icon_name: icon_for(self.running, self.clients.len()).into(),
            icon_pixmap: vec![],
            title: "Wayhelm".into(),
            description,
        }
    }

    fn activate(&mut self, _x: i32, _y: i32) {
        let _ = self.tx.try_send(TrayCmd::Show);
    }

    fn menu(&self) -> Vec<MenuItem<Self>> {
        // The popup itself is already identified as Wayhelm by the host, so
        // the header just shows status (e.g. "Running · 2 clients").
        let mut items: Vec<MenuItem<Self>> = vec![
            StandardItem {
                label: self.status_text.clone(),
                enabled: false,
                ..Default::default()
            }
            .into(),
            MenuItem::Separator,
            StandardItem {
                label: "Show window".into(),
                icon_name: "view-restore-symbolic".into(),
                activate: Box::new(|s: &mut Self| {
                    let _ = s.tx.try_send(TrayCmd::Show);
                }),
                ..Default::default()
            }
            .into(),
        ];

        if self.running {
            items.push(
                StandardItem {
                    label: "Stop wayvnc".into(),
                    icon_name: "media-playback-stop-symbolic".into(),
                    activate: Box::new(|s: &mut Self| {
                        let _ = s.tx.try_send(TrayCmd::Stop);
                    }),
                    ..Default::default()
                }
                .into(),
            );

            // Output switcher submenu. ksni calls menu() every time the user
            // opens the menu, so we get a fresh list of outputs each time
            // without having to push updates from the GTK side.
            let outputs = ctl::output_list().unwrap_or_default();
            if !outputs.is_empty() {
                let submenu: Vec<MenuItem<Self>> = outputs
                    .into_iter()
                    .map(|o| {
                        let name = o.name.clone();
                        let label = if let Some(desc) = o.description.as_deref().filter(|s| !s.is_empty()) {
                            format!("{} — {}", o.name, desc)
                        } else {
                            o.name.clone()
                        };
                        StandardItem {
                            label,
                            icon_name: if o.captured {
                                "object-select-symbolic".into()
                            } else {
                                String::new()
                            },
                            enabled: !o.captured,
                            activate: Box::new(move |_| {
                                let _ = ctl::output_set(&name);
                            }),
                            ..Default::default()
                        }
                        .into()
                    })
                    .collect();
                items.push(
                    SubMenu {
                        label: "Switch output".into(),
                        icon_name: "video-display-symbolic".into(),
                        submenu,
                        ..Default::default()
                    }
                    .into(),
                );
            }
        } else {
            items.push(
                StandardItem {
                    label: "Start wayvnc".into(),
                    icon_name: "media-playback-start-symbolic".into(),
                    activate: Box::new(|s: &mut Self| {
                        let _ = s.tx.try_send(TrayCmd::Start);
                    }),
                    ..Default::default()
                }
                .into(),
            );
        }

        items.push(MenuItem::Separator);
        items.push(
            StandardItem {
                label: "Quit Wayhelm".into(),
                icon_name: "application-exit-symbolic".into(),
                activate: Box::new(|s: &mut Self| {
                    let _ = s.tx.try_send(TrayCmd::Quit);
                }),
                ..Default::default()
            }
            .into(),
        );

        items
    }
}

thread_local! {
    static HANDLE: RefCell<Option<Handle<TrayState>>> = const { RefCell::new(None) };
}

pub fn spawn() -> async_channel::Receiver<TrayCmd> {
    let (tx, rx) = async_channel::unbounded::<TrayCmd>();
    let state = TrayState {
        status_text: "Starting…".into(),
        running: false,
        clients: Vec::new(),
        tx,
    };
    let service = TrayService::new(state);
    let handle = service.handle();
    service.spawn();
    HANDLE.with(|cell| {
        *cell.borrow_mut() = Some(handle);
    });
    rx
}

pub fn update_status(running: bool, client_labels: &[String]) {
    let n = client_labels.len();
    let status_text = if !running {
        "Stopped".to_string()
    } else if n == 0 {
        "Running · no clients".to_string()
    } else if n == 1 {
        "Running · 1 client".to_string()
    } else {
        format!("Running · {n} clients")
    };
    let labels_owned: Vec<String> = client_labels.to_vec();
    HANDLE.with(|cell| {
        if let Some(h) = cell.borrow().as_ref() {
            h.update(move |s| {
                s.running = running;
                s.status_text = status_text;
                s.clients = labels_owned;
            });
        }
    });
}
