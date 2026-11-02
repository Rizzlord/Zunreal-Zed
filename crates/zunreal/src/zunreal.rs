use anyhow::Result;
use gpui::*;
use project::{Project, TaskSourceKind};
use settings::Settings;
use std::sync::Arc;
use ui::prelude::*;
use workspace::Workspace;
use workspace::dock::{DockPosition, Panel, PanelEvent};

#[derive(Clone, Default, Debug)]
pub struct ZunrealSettings {
    pub engine_path: Option<String>,
}

#[cfg(target_os = "linux")]
const CURRENT_PLATFORM: &str = "Linux";
#[cfg(target_os = "windows")]
const CURRENT_PLATFORM: &str = "Win64";
#[cfg(not(any(target_os = "linux", target_os = "windows")))]
const CURRENT_PLATFORM: &str = "Unknown";

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

use task::{RevealTarget, TaskContext, TaskTemplate};

impl ZunrealPanel {
    pub fn new(
        workspace: &mut Workspace,
        _window: &mut Window,
        cx: &mut Context<Workspace>,
    ) -> Entity<Self> {
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
            })
            .detach();
            cx.subscribe(&project, |this, _, event, cx| match event {
                project::Event::WorktreeAdded(_) | project::Event::WorktreeUpdatedEntries(_, _) => {
                    this.detect_uproject(cx);
                }
                _ => {}
            })
            .detach();
            this
        })
    }

    fn detect_uproject(&mut self, cx: &mut Context<Self>) {
        if self.uproject_path.is_some() {
            return;
        }

        let worktrees = self
            .project
            .read(cx)
            .visible_worktrees(cx)
            .collect::<Vec<_>>();
        let mut found = false;
        for worktree in worktrees {
            let worktree = worktree.read(cx);
            // Search root entries first
            for entry in worktree.entries(false, 0) {
                if entry.path.extension() == Some("uproject") {
                    self.uproject_path =
                        Some(worktree.abs_path().join(entry.path.as_std_path()).into());
                    self.project_name = entry.path.file_stem().map(|s| s.to_string());
                    found = true;
                    break;
                }
            }
            if found {
                break;
            }
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
            workspace.update_in(cx, |workspace, window, cx| Self::new(workspace, window, cx))
        })
    }

    pub fn register(cx: &mut App) {
        ZunrealSettings::register(cx);
    }
    fn render_action_button(
        &self,
        label: SharedString,
        tool: &'static str,
        args: Vec<String>,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let tool = tool.to_string();
        let args = args.clone();
        let uproject_path = self.uproject_path.clone();
        let workspace_handle = self.workspace.clone();
        let project_name = self
            .project_name
            .as_deref()
            .unwrap_or("Unknown")
            .to_string();

        div()
            .w_full()
            .id(label.clone())
            .child(
                h_flex()
                    .w_full()
                    .justify_center()
                    .py_1()
                    .child(Label::new(label.clone()).size(LabelSize::Small)),
            )
            .on_click(move |_, window: &mut Window, cx| {
                if let Some(path) = uproject_path.clone() {
                    schedule_unreal_task(
                        workspace_handle.clone(),
                        Some(project_name.clone()),
                        window,
                        cx,
                        &tool,
                        args.clone(),
                        path,
                    );
                }
            })
            .cursor_pointer()
            .bg(cx.theme().colors().editor_background)
            .hover(|style: StyleRefinement| style.bg(cx.theme().colors().element_active))
            .border_1()
            .border_color(cx.theme().colors().border)
            .rounded_md()
    }
}

pub fn schedule_unreal_task(
    workspace_handle: WeakEntity<Workspace>,
    project_name: Option<String>,
    window: &mut Window,
    cx: &mut App,
    command: &str,
    args: Vec<String>,
    uproject_path: Arc<std::path::Path>,
) {
    let Some(workspace) = workspace_handle.upgrade() else {
        return;
    };
    let project_name = project_name.as_deref().unwrap_or("Unknown");
    let settings = ZunrealSettings::get_global(cx);
    let template = unreal_task_template(project_name, settings, command, args, &uproject_path);

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

#[cfg(target_os = "linux")]
fn quote_unreal_argument(argument: &str) -> String {
    format!("'{}'", argument.replace('\'', "'\\''"))
}

fn unreal_task_template(
    project_name: &str,
    settings: &ZunrealSettings,
    command: &str,
    args: Vec<String>,
    uproject_path: &std::path::Path,
) -> TaskTemplate {
    let template_label = format!("Zunreal: {} ({})", command, project_name);

    let mut full_command = command.to_string();
    if let Some(engine_path) = &settings.engine_path {
        let engine_path = std::path::Path::new(engine_path);
        if command == "UnrealBuildTool" {
            #[cfg(target_os = "linux")]
            {
                full_command = quote_unreal_argument(
                    &engine_path
                        .join("Engine/Build/BatchFiles/Linux/Build.sh")
                        .to_string_lossy(),
                );
            }
            #[cfg(target_os = "windows")]
            {
                // On Windows, use single quotes for PowerShell paths to handle spaces reliably.
                full_command = engine_path
                    .join("Engine/Build/BatchFiles/Build.bat")
                    .to_string_lossy()
                    .to_string();
            }
        } else if command == "UnrealEditor" {
            #[cfg(target_os = "linux")]
            {
                let is_wayland = std::env::var("XDG_SESSION_TYPE")
                    .map(|s| s == "wayland")
                    .unwrap_or(false);
                let env_prefix = if is_wayland {
                    "env SDL_VIDEODRIVER=x11 QT_QPA_PLATFORM=xcb SDL_VIDEO_X11_FORCE_EGL=1 "
                } else {
                    ""
                };
                full_command = format!(
                    "{}{}",
                    env_prefix,
                    quote_unreal_argument(
                        &engine_path
                            .join("Engine/Binaries/Linux/UnrealEditor")
                            .to_string_lossy()
                    )
                );
            }
            #[cfg(target_os = "windows")]
            {
                // On Windows, use single quotes for PowerShell paths to handle spaces reliably.
                full_command = engine_path
                    .join("Engine/Binaries/Win64/UnrealEditor.exe")
                    .to_string_lossy()
                    .to_string();
            }
        }
    }

    // Zed's task runner joins arguments as shell source without quoting them.
    #[cfg(target_os = "linux")]
    let mut task_args = args
        .iter()
        .map(|argument| quote_unreal_argument(argument))
        .collect::<Vec<_>>();
    #[cfg(not(target_os = "linux"))]
    let mut task_args = args;

    // Add symlink logic for IntelliSense generation to ensure Zed's clangd picks it up
    if command == "UnrealBuildTool"
        && task_args
            .iter()
            .any(|arg| arg.contains("GenerateClangDatabase"))
    {
        if let Some(engine_path) = &settings.engine_path {
            let engine_path = std::path::Path::new(engine_path);
            let engine_cc_json = engine_path.join("compile_commands.json");
            let project_cc_json = uproject_path
                .parent()
                .unwrap_or(std::path::Path::new("."))
                .join("compile_commands.json");

            // For composite commands, we must include the arguments in the first part of the command
            #[cfg(target_os = "linux")]
            {
                full_command = format!(
                    "{} {} && ln -sf -- {} {}",
                    full_command,
                    task_args.join(" "),
                    quote_unreal_argument(&engine_cc_json.to_string_lossy()),
                    quote_unreal_argument(&project_cc_json.to_string_lossy())
                );
                task_args = vec![];
            }
            #[cfg(target_os = "windows")]
            {
                // On Windows, use an explicit PowerShell command for the composite task.
                // We wrap the whole logic in powershell -Command to avoid Zed's auto-quoting conflicts.
                let quoted_args: Vec<String> = task_args
                    .iter()
                    .map(|arg| {
                        if arg.contains(' ') && !arg.starts_with('\"') && !arg.starts_with('\'') {
                            format!("'{}'", arg)
                        } else {
                            arg.clone()
                        }
                    })
                    .collect();

                let pwsh_command = format!(
                    "& '{}' {} ; cmd /c mklink '{}' '{}'",
                    full_command,
                    quoted_args.join(" "),
                    project_cc_json.to_string_lossy(),
                    engine_cc_json.to_string_lossy()
                );

                full_command = "powershell".to_string();
                task_args = vec![
                    "-NoProfile".to_string(),
                    "-ExecutionPolicy".to_string(),
                    "Bypass".to_string(),
                    "-Command".to_string(),
                    pwsh_command,
                ];
            }
        }
    }

    #[cfg(target_os = "windows")]
    let (final_command, final_args) = {
        if command == "UnrealBuildTool" && template_label.contains("Generate IntelliSense") {
            // Already handled composite command formatting in the IntelliSense block above
            (full_command, vec![])
        } else {
            // For simple commands on Windows, wrap the path in & '...' for PowerShell.
            // This was confirmed working for the "Generate Project Files" and "Build" tasks.
            let quoted_args: Vec<String> = task_args
                .iter()
                .map(|arg| {
                    if arg.contains(' ') && !arg.starts_with('\"') && !arg.starts_with('\'') {
                        format!("\"{}\"", arg)
                    } else {
                        arg.clone()
                    }
                })
                .collect();
            (format!("& '{}'", full_command), quoted_args)
        }
    };
    #[cfg(not(target_os = "windows"))]
    let (final_command, final_args) = (full_command, task_args);

    TaskTemplate {
        label: template_label,
        command: final_command,
        args: final_args,
        reveal_target: RevealTarget::Dock,
        // The Linux command uses POSIX quoting and shell operators, regardless of the user
        // terminal shell (which may be fish or Nushell).
        #[cfg(target_os = "linux")]
        shell: task::Shell::Program("/bin/bash".to_string()),
        ..Default::default()
    }
}

impl Render for ZunrealPanel {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let settings = ZunrealSettings::get_global(cx);
        let has_engine_path = settings.engine_path.is_some();
        let project_name = self
            .project_name
            .as_deref()
            .unwrap_or("No Project Detected");

        v_flex()
            .track_focus(&self.focus_handle)
            .bg(cx.theme().colors().panel_background)
            .size_full()
            .p_4()
            .gap_4()
            .child(
                v_flex()
                    .child(
                        Label::new("Unreal Engine")
                            .size(LabelSize::Large)
                            .color(Color::Default),
                    )
                    .child(
                        Label::new(format!("Project: {}", project_name))
                            .size(LabelSize::Small)
                            .color(Color::Muted),
                    ),
            )
            .when(!has_engine_path, |this| {
                this.child(
                    v_flex()
                        .gap_1()
                        .p_2()
                        .bg(rgb(0x2a2a2a))
                        .border_1()
                        .border_color(cx.theme().status().error)
                        .child(
                            Label::new("Engine path not set in settings")
                                .color(Color::Error)
                                .size(LabelSize::Small),
                        )
                        .child(
                            Label::new(
                                "Add 'zunreal': { 'engine_path': '...' } to your settings.json",
                            )
                            .size(LabelSize::XSmall)
                            .color(Color::Muted),
                        ),
                )
            })
            .child(
                v_flex()
                    .gap_2()
                    .child(self.render_action_button(
                        "Build".into(),
                        "UnrealBuildTool",
                        vec![
                                format!("{}Editor", self.project_name.clone().unwrap_or_default()),
                                CURRENT_PLATFORM.to_string(),
                                "Development".to_string(),
                                self.uproject_path
                                    .as_ref()
                                    .map(|p| p.to_string_lossy().into_owned())
                                    .unwrap_or_default(),
                            ],
                        cx,
                    ))
                    .child(self.render_action_button(
                        "Open Editor".into(),
                        "UnrealEditor",
                        vec![
                                self.uproject_path
                                    .as_ref()
                                    .map(|p| p.to_string_lossy().into_owned())
                                    .unwrap_or_default(),
                            ],
                        cx,
                    ))
                    .child(self.render_action_button(
                        "Cook".into(),
                        "UnrealEditor",
                        vec![
                                self.uproject_path
                                    .as_ref()
                                    .map(|p| p.to_string_lossy().into_owned())
                                    .unwrap_or_default(),
                                "-run=Cook".to_string(),
                                format!("-targetplatform={}", CURRENT_PLATFORM),
                            ],
                        cx,
                    ))
                    .child(self.render_action_button(
                        "Run Game".into(),
                        "UnrealEditor",
                        vec![
                                self.uproject_path
                                    .as_ref()
                                    .map(|p| p.to_string_lossy().into_owned())
                                    .unwrap_or_default(),
                                "-game".to_string(),
                            ],
                        cx,
                    ))
                    .child(self.render_action_button(
                        "Generate Project Files".into(),
                        "UnrealBuildTool",
                        vec![
                            "-projectfiles".to_string(),
                            format!(
                                    "-project={}",
                                    self.uproject_path
                                        .as_ref()
                                        .map(|p| p.to_string_lossy().into_owned())
                                        .unwrap_or_default()
                                ),
                        ],
                        cx,
                    ))
                    .child(self.render_action_button(
                        "Generate IntelliSense".into(),
                        "UnrealBuildTool",
                        vec![
                            format!("{}Editor", self.project_name.clone().unwrap_or_default()),
                            CURRENT_PLATFORM.to_string(),
                            "Development".to_string(),
                            "-mode=GenerateClangDatabase".to_string(),
                            format!(
                                    "-project={}",
                                    self.uproject_path
                                        .as_ref()
                                        .map(|p| p.to_string_lossy().into_owned())
                                        .unwrap_or_default()
                                ),
                            "-game".to_string(),
                            "-engine".to_string(),
                        ],
                        cx,
                    )),
            )
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

    fn set_position(
        &mut self,
        _position: DockPosition,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) {
    }

    fn default_size(&self, _window: &Window, _cx: &App) -> Pixels {
        px(300.)
    }

    fn icon(&self, _window: &Window, _cx: &App) -> Option<IconName> {
        Some(IconName::Unreal)
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
    cx.observe_new(|workspace: &mut Workspace, _, _| {
        workspace.register_action(|workspace, _: &ToggleZunrealPanel, window, cx| {
            workspace.toggle_panel_focus::<ZunrealPanel>(window, cx);
        });
    })
    .detach();
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::{ZunrealSettings, unreal_task_template};
    use anyhow::{Context as _, Result};
    use std::os::unix::fs::PermissionsExt;
    use std::path::{Path, PathBuf};
    use std::process::{Command, Output};
    use task::TaskContext;

    struct MockUnreal {
        _directory: tempfile::TempDir,
        engine_path: PathBuf,
        project_path: PathBuf,
    }

    impl MockUnreal {
        fn new() -> Result<Self> {
            let directory = tempfile::tempdir()?;
            let engine_path = directory
                .path()
                .join("Unreal Engine's $cash $(printf BAD) \"5\"");
            let project_directory = directory
                .path()
                .join("Unreal Projects/O'Brien $cash $(printf BAD)");
            std::fs::create_dir_all(&project_directory)?;
            for relative_path in [
                "Engine/Build/BatchFiles/Linux/Build.sh",
                "Engine/Binaries/Linux/UnrealEditor",
            ] {
                let executable = engine_path.join(relative_path);
                std::fs::create_dir_all(executable.parent().context("missing mock parent")?)?;
                std::fs::write(
                    &executable,
                    "#!/bin/sh\nprintf '%s\\0' \"$@\"\nexit \"${ZUNREAL_TEST_EXIT:-0}\"\n",
                )?;
                std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o700))?;
            }
            Ok(Self {
                engine_path,
                project_path: project_directory.join("LifeSeekerAKR.uproject"),
                _directory: directory,
            })
        }

        fn run(&self, tool: &str, arguments: Vec<String>, exit_code: &str) -> Result<Output> {
            let settings = ZunrealSettings {
                engine_path: Some(self.engine_path.to_string_lossy().into_owned()),
            };
            let template = unreal_task_template(
                "LifeSeekerAKR",
                &settings,
                tool,
                arguments,
                &self.project_path,
            );
            let task = template
                .resolve_task("zunreal-test", &TaskContext::default())
                .context("failed to resolve mock task")?
                .resolved;
            let (program, arguments) = task::ShellBuilder::new(&task.shell, false)
                .non_interactive()
                .build_no_quote(task.command, &task.args);
            Ok(Command::new(program)
                .args(arguments)
                .env("ZUNREAL_TEST_EXIT", exit_code)
                .output()?)
        }

        fn project_database(&self) -> Result<PathBuf> {
            Ok(self
                .project_path
                .parent()
                .context("missing project parent")?
                .join("compile_commands.json"))
        }
    }

    fn assert_arguments(output: &Output, expected: &[String]) {
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let expected = expected
            .iter()
            .flat_map(|argument| argument.bytes().chain([0]))
            .collect::<Vec<_>>();
        assert_eq!(output.stdout, expected);
    }

    #[test]
    fn unreal_actions_preserve_literal_arguments() -> Result<()> {
        let mock = MockUnreal::new()?;
        let project = mock.project_path.to_string_lossy().into_owned();
        let actions = [
            (
                "UnrealBuildTool",
                vec![
                    "LifeSeekerAKREditor".into(),
                    "Linux".into(),
                    "Development".into(),
                    project.clone(),
                ],
            ),
            ("UnrealEditor", vec![project.clone()]),
            (
                "UnrealEditor",
                vec![
                    project.clone(),
                    "-run=Cook".into(),
                    "-targetplatform=Linux".into(),
                ],
            ),
            ("UnrealEditor", vec![project.clone(), "-game".into()]),
            (
                "UnrealBuildTool",
                vec!["-projectfiles".into(), format!("-project={project}")],
            ),
            (
                "UnrealEditor",
                vec![
                    project,
                    "".into(),
                    "a\nb\t'\"$HOME`printf BAD`; printf BAD".into(),
                ],
            ),
        ];
        for (tool, arguments) in actions {
            assert_arguments(&mock.run(tool, arguments.clone(), "0")?, &arguments);
        }
        Ok(())
    }

    fn intellisense_arguments(project: &Path) -> Vec<String> {
        vec![
            "LifeSeekerAKREditor".into(),
            "Linux".into(),
            "Development".into(),
            "-mode=GenerateClangDatabase".into(),
            format!("-project={}", project.display()),
            "-game".into(),
            "-engine".into(),
        ]
    }

    #[test]
    fn intellisense_preserves_arguments_and_links_database() -> Result<()> {
        let mock = MockUnreal::new()?;
        let database = mock.engine_path.join("compile_commands.json");
        std::fs::write(&database, "[]")?;
        let arguments = intellisense_arguments(&mock.project_path);
        assert_arguments(
            &mock.run("UnrealBuildTool", arguments.clone(), "0")?,
            &arguments,
        );
        assert_eq!(std::fs::read_link(mock.project_database()?)?, database);
        Ok(())
    }

    #[test]
    fn failed_intellisense_does_not_link_database() -> Result<()> {
        let mock = MockUnreal::new()?;
        let output = mock.run(
            "UnrealBuildTool",
            intellisense_arguments(&mock.project_path),
            "7",
        )?;
        assert_eq!(output.status.code(), Some(7));
        assert!(!mock.project_database()?.try_exists()?);
        assert!(
            matches!(std::fs::symlink_metadata(mock.project_database()?), Err(error) if error.kind() == std::io::ErrorKind::NotFound)
        );
        Ok(())
    }
}
