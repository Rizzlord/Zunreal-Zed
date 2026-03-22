use gpui::*;
use ui::{prelude::*, ButtonLike};
use workspace::dock::{Panel, PanelEvent, DockPosition};
use workspace::{Workspace, StatusItemView, ItemHandle};
use project::{Project, TaskSourceKind};
use gpui_util::ResultExt;
use settings::Settings;
use std::sync::Arc;
use anyhow::Result;

#[derive(Clone, Default, Debug)]
pub struct ZunrealSettings {
    pub engine_path: Option<String>,
}

impl Settings for ZunrealSettings {
    fn from_settings(content: &settings::SettingsContent) -> Self {
        let zunreal = content.zunreal.as_ref();
        Self {
            engine_path: zunreal.and_then(|z| z.engine_path.clone()),
        }
    }
}

pub struct ZunrealPanel {
    workspace: WeakEntity<Workspace>,
    project: Entity<Project>,
    focus_handle: FocusHandle,
    uproject_path: Option<Arc<std::path::Path>>,
    project_name: Option<String>,
}

use task::{TaskTemplate, TaskContext, RevealTarget};

impl ZunrealPanel {
    pub fn new(workspace: &mut Workspace, _window: &mut Window, cx: &mut Context<Workspace>) -> Entity<Self> {
        let project = workspace.project().clone();
        let workspace_handle = cx.entity().downgrade();
        cx.new(|cx: &mut Context<ZunrealPanel>| {
            let mut this = Self {
                workspace: workspace_handle,
                project: project.clone(),
                focus_handle: cx.focus_handle(),
                uproject_path: None,
                project_name: None,
            };
            this.detect_uproject(cx);
            cx.observe(&project, |this, _, cx| {
                this.detect_uproject(cx);
            }).detach();
            cx.subscribe(&project, |this, _, event, cx| {
                match event {
                    project::Event::WorktreeAdded(_) | project::Event::WorktreeUpdatedEntries(_, _) => {
                        this.detect_uproject(cx);
                    }
                    _ => {}
                }
            }).detach();
            this
        })
    }

    fn detect_uproject(&mut self, cx: &mut Context<Self>) {
        if self.uproject_path.is_some() {
            return;
        }

        let worktrees = self.project.read(cx).visible_worktrees(cx).collect::<Vec<_>>();
        let mut found = false;
        for worktree in worktrees {
            let worktree = worktree.read(cx);
            // Search root entries first
            for entry in worktree.entries(false, 0) {
                if entry.path.extension() == Some("uproject") {
                    self.uproject_path = Some(worktree.abs_path().join(entry.path.as_std_path()).into());
                    self.project_name = entry.path.file_stem().map(|s| s.to_string());
                    found = true;
                    break;
                }
            }
            if found { break; }
        }

        if found {
            cx.notify();
        }
    }

    pub fn load(
        workspace: WeakEntity<Workspace>,
        cx: &mut AsyncWindowContext,
    ) -> Task<Result<Entity<Self>>> {
        cx.spawn(async move |cx| {
            workspace.update_in(cx, |workspace, window, cx| {
                Self::new(workspace, window, cx)
            })
        })
    }

    pub fn register(cx: &mut App) {
        ZunrealSettings::register(cx);
    }

    pub fn schedule_unreal_task(&self, label: &str, command: &str, args: Vec<String>, window: &mut Window, cx: &mut Context<Self>) {
        let Some(workspace) = self.workspace.upgrade() else { return };
        let project_name = self.project_name.as_deref().unwrap_or("Unknown");
        let settings = ZunrealSettings::get_global(cx);
        
        let mut full_command = command.to_string();
        if let Some(engine_path) = &settings.engine_path {
            let engine_path = std::path::Path::new(engine_path);
            if command == "UnrealBuildTool" {
                #[cfg(target_os = "linux")]
                {
                    full_command = format!("\"{}\"", engine_path.join("Engine/Build/BatchFiles/Linux/Build.sh").to_string_lossy());
                }
                #[cfg(target_os = "windows")]
                {
                    full_command = format!("\"{}\"", engine_path.join("Engine/Build/BatchFiles/Build.bat").to_string_lossy());
                }
            } else if command == "UnrealEditor" {
                #[cfg(target_os = "linux")]
                {
                    let is_wayland = std::env::var("XDG_SESSION_TYPE").map(|s| s == "wayland").unwrap_or(false);
                    let env_prefix = if is_wayland {
                        "env SDL_VIDEODRIVER=x11 QT_QPA_PLATFORM=xcb SDL_VIDEO_X11_FORCE_EGL=1 "
                    } else {
                        ""
                    };
                    full_command = format!("{}\"{}\"", env_prefix, engine_path.join("Engine/Binaries/Linux/UnrealEditor").to_string_lossy());
                }
                #[cfg(target_os = "windows")]
                {
                    full_command = format!("\"{}\"", engine_path.join("Engine/Binaries/Win64/UnrealEditor.exe").to_string_lossy());
                }
            }
        }
        
        let template = TaskTemplate {
            label: format!("Zunreal: {} ({})", label, project_name),
            command: full_command,
            args,
            reveal_target: RevealTarget::Dock,
            ..Default::default()
        };

        workspace.update(cx, |workspace, cx| {
            workspace.schedule_task(
                TaskSourceKind::UserInput,
                &template,
                &TaskContext::default(),
                false,
                window,
                cx,
            );
        });
    }

    fn render_action_button(&self, label: &str, command: &str, args: Vec<String>, cx: &mut Context<Self>) -> impl IntoElement {
        let label = label.to_string();
        let command = command.to_string();
        let uproject_path = self.uproject_path.clone();
        Button::new(label.to_lowercase(), label.clone())
            .on_click(cx.listener({
                let label = label.clone();
                move |this, _, window, cx| {
                    if let Some(_) = &uproject_path {
                        this.schedule_unreal_task(&label, &command, args.clone(), window, cx);
                    }
                }
            }))
            .full_width()
            .when(label == "Build" || label == "Open Editor" || label == "Run Game", |this| this.style(ButtonStyle::Filled))
    }
}

impl Render for ZunrealPanel {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let settings = ZunrealSettings::get_global(cx);
        let has_engine_path = settings.engine_path.is_some();
        let project_name = self.project_name.as_deref().unwrap_or("No Project Detected");

        v_flex()
            .bg(rgb(0x121212)) // UE charcoal
            .size_full()
            .p_4()
            .gap_4()
            .child(
                v_flex()
                    .child(Label::new("ZUNREAL").size(LabelSize::Large).color(Color::Accent))
                    .child(Label::new(format!("Project: {}", project_name)).size(LabelSize::Small).color(Color::Muted))
            )
            .when(!has_engine_path, |this| {
                this.child(
                    v_flex()
                        .gap_1()
                        .p_2()
                        .bg(rgb(0x2a2a2a))
                        .border_1()
                        .border_color(cx.theme().status().error)
                        .child(Label::new("Engine path not set in settings").color(Color::Error).size(LabelSize::Small))
                        .child(Label::new("Add 'zunreal': { 'engine_path': '...' } to your settings.json").size(LabelSize::XSmall).color(Color::Muted))
                )
            })
            .child(
                v_flex()
                    .gap_2()
                    .child(self.render_action_button("Build", "UnrealBuildTool", vec![ format!("{}Editor", self.project_name.clone().unwrap_or_default()), "Linux".to_string(), "Development".to_string(), format!("\"{}\"", self.uproject_path.as_ref().map(|p| p.to_string_lossy().into_owned()).unwrap_or_default()) ], cx))
                    .child(self.render_action_button("Open Editor", "UnrealEditor", vec![ format!("\"{}\"", self.uproject_path.as_ref().map(|p| p.to_string_lossy().into_owned()).unwrap_or_default()) ], cx))
                    .child(self.render_action_button("Run Game", "UnrealEditor", vec![ format!("\"{}\"", self.uproject_path.as_ref().map(|p| p.to_string_lossy().into_owned()).unwrap_or_default()), "-game".to_string() ], cx))
                    .child(self.render_action_button("Cook", "UnrealEditor", vec![ format!("\"{}\"", self.uproject_path.as_ref().map(|p| p.to_string_lossy().into_owned()).unwrap_or_default()), "-run=Cook".to_string(), "-targetplatform=Linux".to_string() ], cx))
                    .child(self.render_action_button("Generate Project Files", "UnrealBuildTool", vec![ "-projectfiles".to_string(), format!("-project=\"{}\"", self.uproject_path.as_ref().map(|p| p.to_string_lossy().into_owned()).unwrap_or_default()) ], cx))
            )
    }
}

pub struct ZunrealStatusIndicator;

impl ZunrealStatusIndicator {
    pub fn new() -> Self {
        Self
    }
}

impl Render for ZunrealStatusIndicator {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        ButtonLike::new("zunreal-status")
            .child(
                h_flex()
                    .bg(rgb(0x0070FF)) // Unreal Blue
                    .rounded_md()
                    .px_2()
                    .child(Label::new("Zunreal").size(LabelSize::Small).color(Color::Default))
            )
            .on_click(move |_, window: &mut Window, cx| {
                window.dispatch_action(ToggleZunrealPanel.boxed_clone(), cx);
            })
    }
}

impl StatusItemView for ZunrealStatusIndicator {
    fn set_active_pane_item(
        &mut self,
        _active_pane_item: Option<&dyn ItemHandle>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) {
    }
}

impl Focusable for ZunrealPanel {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl EventEmitter<PanelEvent> for ZunrealPanel {}

impl Panel for ZunrealPanel {
    fn persistent_name() -> &'static str {
        "ZunrealPanel"
    }

    fn panel_key() -> &'static str {
        "zunreal_panel"
    }

    fn position(&self, _window: &Window, _cx: &App) -> DockPosition {
        DockPosition::Right
    }

    fn position_is_valid(&self, position: DockPosition) -> bool {
        matches!(position, DockPosition::Right | DockPosition::Left)
    }

    fn set_position(&mut self, _position: DockPosition, _window: &mut Window, _cx: &mut Context<Self>) {
    }

    fn size(&self, _window: &Window, _cx: &App) -> Pixels {
        px(300.)
    }

    fn set_size(&mut self, _size: Option<Pixels>, _window: &mut Window, _cx: &mut Context<Self>) {
    }

    fn icon(&self, _window: &Window, _cx: &App) -> Option<IconName> {
        Some(IconName::ToolHammer) // Placeholder for Unreal icon
    }

    fn icon_tooltip(&self, _window: &Window, _cx: &App) -> Option<&'static str> {
        Some("Zunreal Project Panel")
    }

    fn toggle_action(&self) -> Box<dyn Action> {
        Box::new(ToggleZunrealPanel)
    }

    fn activation_priority(&self) -> u32 {
        100
    }
}

#[derive(Clone, PartialEq, serde::Deserialize, gpui::Action)]
pub struct ToggleZunrealPanel;

pub fn init(cx: &mut App) {
    ZunrealSettings::register(cx);
    cx.observe_new(|workspace: &mut Workspace, window, cx| {
        let workspace_handle = cx.entity().downgrade();
        if let Some(window) = window {
            cx.spawn_in(window, move |_, window_cx: &mut AsyncWindowContext| {
                let mut window_cx = window_cx.clone();
                async move {
                    ZunrealPanel::load(workspace_handle, &mut window_cx)
                        .await
                        .log_err();
                }
            })
            .detach();
        }

        workspace.register_action(|workspace, _: &ToggleZunrealPanel, window, cx| {
            workspace.toggle_panel_focus::<ZunrealPanel>(window, cx);
        });
    })
    .detach();
}
