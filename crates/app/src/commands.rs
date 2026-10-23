//! Every user-facing command with its label, icon and shortcut. The toolbar,
//! context menu, tooltips and the command palette all read from here, so a
//! shortcut is written down exactly once.

use crate::icons as i;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Cmd {
    Open,
    QuickLook,
    EnclosingFolder,
    Back,
    Forward,
    Refresh,
    CopyToOther,
    MoveToOther,
    Rename,
    NewFolder,
    /// ⌘⌫: local items go to the Trash; on servers (no Trash) this deletes
    /// permanently after asking.
    MoveToTrash,
    /// ⌥⌘⌫: delete permanently after asking.
    DeletePermanently,
    /// ⇧⌘⌫: delete everything in the Trash after asking (Finder).
    EmptyTrash,
    SelectAll,
    ToggleHidden,
    NewTab,
    CloseTab,
    SwitchPane,
    /// Add a pane right of the active one, showing the same folder.
    SplitPane,
    /// Close the active pane (not the last one).
    ClosePane,
    /// Focus the pane to the left / right.
    FocusPaneLeft,
    FocusPaneRight,
    EditPath,
    GoToPath,
    Filter,
    AddToFavorites,
    Connect,
    Transfers,
    CompareSync,
    CommandPalette,
    Settings,
    /// The user manual (⌘?).
    Help,
    /// "Manta Website & Support…": the website in the browser.
    Support,
    /// "Source Code on GitHub…": the repository in the browser.
    SourceCode,
    /// "Report an Issue…": GitHub's bug report form, prefilled with the
    /// Manta and macOS versions.
    ReportIssue,
    /// "Suggest an Idea…": GitHub's feature request form.
    SuggestIdea,
    /// The licence notices of the components Manta is built on.
    ThirdPartyLicenses,
    /// Restart external plugins and rescan the plugins folder.
    ReloadPlugins,
    /// The plugin directory on the website.
    BrowsePlugins,
    Compress,
    ExtractHere,
    OpenWithApp,
    /// Interactive shell inside the container shown in the pane.
    OpenShellInContainer,
    AppearanceLight,
    AppearanceDark,
    AppearanceSystem,
    /// The active pane's view (⌘1 – ⌘4).
    ViewIcons,
    ViewList,
    ViewColumns,
    ViewGallery,
}

pub const ALL: &[Cmd] = &[
    Cmd::Open,
    Cmd::QuickLook,
    Cmd::EnclosingFolder,
    Cmd::Back,
    Cmd::Forward,
    Cmd::Refresh,
    Cmd::CopyToOther,
    Cmd::MoveToOther,
    Cmd::Rename,
    Cmd::NewFolder,
    Cmd::MoveToTrash,
    Cmd::DeletePermanently,
    Cmd::EmptyTrash,
    Cmd::SelectAll,
    Cmd::ToggleHidden,
    Cmd::NewTab,
    Cmd::CloseTab,
    Cmd::SwitchPane,
    Cmd::SplitPane,
    Cmd::ClosePane,
    Cmd::FocusPaneLeft,
    Cmd::FocusPaneRight,
    Cmd::EditPath,
    Cmd::GoToPath,
    Cmd::Filter,
    Cmd::AddToFavorites,
    Cmd::Connect,
    Cmd::Transfers,
    Cmd::CompareSync,
    Cmd::Settings,
    Cmd::Help,
    Cmd::Support,
    Cmd::SourceCode,
    Cmd::ReportIssue,
    Cmd::SuggestIdea,
    Cmd::ThirdPartyLicenses,
    Cmd::ReloadPlugins,
    Cmd::BrowsePlugins,
    Cmd::Compress,
    Cmd::ExtractHere,
    Cmd::OpenWithApp,
    Cmd::OpenShellInContainer,
    Cmd::AppearanceLight,
    Cmd::AppearanceDark,
    Cmd::AppearanceSystem,
    Cmd::ViewIcons,
    Cmd::ViewList,
    Cmd::ViewColumns,
    Cmd::ViewGallery,
];

impl Cmd {
    /// Commands that work between two panes: off with a single pane.
    pub fn needs_two_panes(self) -> bool {
        matches!(
            self,
            Cmd::CopyToOther
                | Cmd::MoveToOther
                | Cmd::CompareSync
                | Cmd::SwitchPane
                | Cmd::ClosePane
                | Cmd::FocusPaneLeft
                | Cmd::FocusPaneRight
        )
    }

    pub fn label(self) -> &'static str {
        match self {
            Cmd::Open => t!("cmd.open"),
            Cmd::QuickLook => t!("cmd.quick_look"),
            Cmd::EnclosingFolder => t!("cmd.enclosing_folder"),
            Cmd::Back => t!("cmd.back"),
            Cmd::Forward => t!("cmd.forward"),
            Cmd::Refresh => t!("cmd.refresh"),
            Cmd::CopyToOther => t!("cmd.copy_to_other"),
            Cmd::MoveToOther => t!("cmd.move_to_other"),
            Cmd::Rename => t!("cmd.rename"),
            Cmd::NewFolder => t!("cmd.new_folder"),
            Cmd::MoveToTrash => t!("cmd.move_to_trash"),
            Cmd::DeletePermanently => t!("cmd.delete_permanently"),
            Cmd::EmptyTrash => t!("cmd.empty_trash"),
            Cmd::SelectAll => t!("cmd.select_all"),
            Cmd::ToggleHidden => t!("cmd.toggle_hidden"),
            Cmd::NewTab => t!("cmd.new_tab"),
            Cmd::CloseTab => t!("cmd.close_tab"),
            Cmd::SwitchPane => t!("cmd.switch_pane"),
            Cmd::SplitPane => t!("cmd.split_pane"),
            Cmd::ClosePane => t!("cmd.close_pane"),
            Cmd::FocusPaneLeft => t!("cmd.focus_pane_left"),
            Cmd::FocusPaneRight => t!("cmd.focus_pane_right"),
            Cmd::EditPath => t!("cmd.edit_path"),
            Cmd::GoToPath => t!("cmd.go_to_path"),
            Cmd::Filter => t!("cmd.filter"),
            Cmd::AddToFavorites => t!("cmd.add_to_favorites"),
            Cmd::Connect => t!("cmd.connect"),
            Cmd::Transfers => t!("cmd.transfers"),
            Cmd::CompareSync => t!("cmd.compare_sync"),
            Cmd::CommandPalette => t!("cmd.command_palette"),
            Cmd::Settings => t!("cmd.settings"),
            Cmd::Help => t!("cmd.help"),
            Cmd::Support => t!("cmd.support"),
            Cmd::SourceCode => t!("cmd.source_code"),
            Cmd::ReportIssue => t!("cmd.report_issue"),
            Cmd::SuggestIdea => t!("cmd.suggest_idea"),
            Cmd::ThirdPartyLicenses => t!("cmd.third_party_licenses"),
            Cmd::ReloadPlugins => t!("cmd.reload_plugins"),
            Cmd::BrowsePlugins => t!("cmd.browse_plugins"),
            Cmd::Compress => t!("cmd.compress"),
            Cmd::ExtractHere => t!("cmd.extract_here"),
            Cmd::OpenWithApp => t!("cmd.open_with_app"),
            Cmd::OpenShellInContainer => t!("cmd.open_shell_in_container"),
            Cmd::AppearanceLight => t!("cmd.appearance_light"),
            Cmd::AppearanceDark => t!("cmd.appearance_dark"),
            Cmd::AppearanceSystem => t!("cmd.appearance_system"),
            Cmd::ViewIcons => t!("cmd.view_icons"),
            Cmd::ViewList => t!("cmd.view_list"),
            Cmd::ViewColumns => t!("cmd.view_columns"),
            Cmd::ViewGallery => t!("cmd.view_gallery"),
        }
    }

    /// Shortcut as shown to the user (macOS glyphs).
    pub fn shortcut(self) -> &'static str {
        match self {
            Cmd::Open => crate::icons::RETURN,
            Cmd::QuickLook => t!("cmd.shortcut.space"),
            Cmd::EnclosingFolder => "⌘↑",
            Cmd::Back => "⌘[",
            Cmd::Forward => "⌘]",
            Cmd::Refresh => "⌘R",
            Cmd::CopyToOther => "F5",
            Cmd::MoveToOther => "F6",
            Cmd::Rename => "F2",
            Cmd::NewFolder => "⇧⌘N",
            Cmd::MoveToTrash => "⌘⌫",
            Cmd::DeletePermanently => "⌥⌘⌫",
            Cmd::EmptyTrash => "⇧⌘⌫",
            Cmd::SelectAll => "⌘A",
            Cmd::ToggleHidden => "⇧⌘.",
            Cmd::NewTab => "⌘T",
            Cmd::CloseTab => "⌘W",
            Cmd::SwitchPane => "⇥",
            Cmd::SplitPane => "⌘D",
            Cmd::ClosePane => "⇧⌘D",
            Cmd::FocusPaneLeft => "⌥⌘←",
            Cmd::FocusPaneRight => "⌥⌘→",
            Cmd::EditPath => "⌘L",
            Cmd::GoToPath => "⇧⌘G",
            Cmd::Filter => "⌘F",
            Cmd::AddToFavorites => "⌃⌘T",
            Cmd::Connect => "⌘K",
            Cmd::Transfers => "⌥⌘L",
            Cmd::CompareSync => "⇧⌘S",
            Cmd::CommandPalette => "⇧⌘P",
            Cmd::Settings => "⌘,",
            Cmd::Help => "⌘?",
            Cmd::Compress => "⌥⌘C",
            Cmd::ExtractHere => "⌥⌘E",
            Cmd::OpenWithApp => "⌥↩",
            Cmd::OpenShellInContainer
            | Cmd::Support
            | Cmd::SourceCode
            | Cmd::ReportIssue
            | Cmd::SuggestIdea
            | Cmd::ThirdPartyLicenses
            | Cmd::ReloadPlugins
            | Cmd::BrowsePlugins => "",
            Cmd::AppearanceLight | Cmd::AppearanceDark | Cmd::AppearanceSystem => "",
            Cmd::ViewIcons => "⌘1",
            Cmd::ViewList => "⌘2",
            Cmd::ViewColumns => "⌘3",
            Cmd::ViewGallery => "⌘4",
        }
    }

    pub fn icon(self) -> &'static str {
        match self {
            Cmd::Open => i::FOLDER_OPEN,
            Cmd::QuickLook => i::EYE,
            Cmd::EnclosingFolder => i::ARROW_UP,
            Cmd::Back => i::CARET_LEFT,
            Cmd::Forward => i::CARET_RIGHT,
            Cmd::Refresh => i::ARROW_CLOCKWISE,
            Cmd::CopyToOther => i::COPY,
            Cmd::MoveToOther => i::ARROW_LINE_RIGHT,
            Cmd::Rename => i::PENCIL_SIMPLE,
            Cmd::NewFolder => i::FOLDER_PLUS,
            Cmd::MoveToTrash => i::TRASH,
            Cmd::DeletePermanently => i::TRASH,
            Cmd::EmptyTrash => i::TRASH,
            Cmd::SelectAll => i::CHECK_CIRCLE,
            Cmd::ToggleHidden => i::FILE_DASHED,
            Cmd::NewTab => i::PLUS,
            Cmd::CloseTab => i::X,
            Cmd::SwitchPane => i::ARROWS_LEFT_RIGHT,
            Cmd::SplitPane => i::COLUMNS_PLUS_RIGHT,
            Cmd::ClosePane => i::X,
            Cmd::FocusPaneLeft => i::ARROW_LEFT,
            Cmd::FocusPaneRight => i::ARROW_RIGHT,
            Cmd::EditPath => i::TERMINAL_WINDOW,
            Cmd::GoToPath => i::SIGNPOST,
            Cmd::Filter => i::FUNNEL_SIMPLE,
            Cmd::AddToFavorites => i::STAR,
            Cmd::Connect => i::GLOBE,
            Cmd::Transfers => i::ARROWS_DOWN_UP,
            Cmd::CompareSync => i::ARROWS_LEFT_RIGHT,
            Cmd::CommandPalette => i::COMMAND,
            Cmd::Settings => i::GEAR_SIX,
            Cmd::Help => i::QUESTION,
            Cmd::Support => i::GLOBE,
            Cmd::SourceCode => i::CODE,
            Cmd::ReportIssue => i::BUG,
            Cmd::SuggestIdea => i::LIGHTBULB,
            Cmd::ThirdPartyLicenses => i::SCROLL,
            Cmd::ReloadPlugins => i::ARROW_CLOCKWISE,
            Cmd::BrowsePlugins => i::PUZZLE_PIECE,
            Cmd::Compress => i::FILE_ARCHIVE,
            Cmd::ExtractHere => i::ARCHIVE,
            Cmd::OpenWithApp => i::ARROW_SQUARE_OUT,
            Cmd::OpenShellInContainer => i::TERMINAL_WINDOW,
            Cmd::AppearanceLight => i::SUN,
            Cmd::AppearanceDark => i::MOON,
            Cmd::AppearanceSystem => i::CIRCLE_HALF,
            Cmd::ViewIcons => i::SQUARES_FOUR,
            Cmd::ViewList => i::LIST,
            Cmd::ViewColumns => i::COLUMNS,
            Cmd::ViewGallery => i::SLIDESHOW,
        }
    }

    pub fn category(self) -> &'static str {
        match self {
            Cmd::Open
            | Cmd::QuickLook
            | Cmd::EnclosingFolder
            | Cmd::Back
            | Cmd::Forward
            | Cmd::Refresh
            | Cmd::EditPath
            | Cmd::GoToPath
            | Cmd::OpenWithApp => t!("cmd.cat.navigate"),
            Cmd::CopyToOther
            | Cmd::MoveToOther
            | Cmd::Rename
            | Cmd::NewFolder
            | Cmd::MoveToTrash
            | Cmd::DeletePermanently
            | Cmd::EmptyTrash
            | Cmd::Compress
            | Cmd::ExtractHere => t!("cmd.cat.file"),
            Cmd::SelectAll | Cmd::Filter => t!("cmd.cat.select"),
            Cmd::ToggleHidden
            | Cmd::NewTab
            | Cmd::CloseTab
            | Cmd::SwitchPane
            | Cmd::SplitPane
            | Cmd::ClosePane
            | Cmd::FocusPaneLeft
            | Cmd::FocusPaneRight
            | Cmd::Transfers
            | Cmd::CommandPalette
            | Cmd::AppearanceLight
            | Cmd::AppearanceDark
            | Cmd::AppearanceSystem
            | Cmd::ViewIcons
            | Cmd::ViewList
            | Cmd::ViewColumns
            | Cmd::ViewGallery => t!("cmd.cat.view"),
            Cmd::AddToFavorites | Cmd::Connect => t!("cmd.cat.go"),
            Cmd::CompareSync
            | Cmd::Settings
            | Cmd::OpenShellInContainer
            | Cmd::Help
            | Cmd::Support
            | Cmd::SourceCode
            | Cmd::ReportIssue
            | Cmd::SuggestIdea
            | Cmd::ThirdPartyLicenses
            | Cmd::ReloadPlugins
            | Cmd::BrowsePlugins => t!("cmd.cat.tools"),
        }
    }

    /// Extra words the palette matches on.
    pub fn keywords(self) -> &'static str {
        match self {
            Cmd::Open => t!("cmd.kw.open"),
            Cmd::QuickLook => t!("cmd.kw.quick_look"),
            Cmd::EnclosingFolder => t!("cmd.kw.enclosing_folder"),
            Cmd::Back => t!("cmd.kw.back"),
            Cmd::Forward => t!("cmd.kw.forward"),
            Cmd::Refresh => t!("cmd.kw.refresh"),
            Cmd::CopyToOther => t!("cmd.kw.copy_to_other"),
            Cmd::MoveToOther => t!("cmd.kw.move_to_other"),
            Cmd::Rename => t!("cmd.kw.rename"),
            Cmd::NewFolder => t!("cmd.kw.new_folder"),
            Cmd::MoveToTrash => t!("cmd.kw.move_to_trash"),
            Cmd::DeletePermanently => t!("cmd.kw.delete_permanently"),
            Cmd::EmptyTrash => t!("cmd.kw.empty_trash"),
            Cmd::SelectAll => t!("cmd.kw.select_all"),
            Cmd::ToggleHidden => t!("cmd.kw.toggle_hidden"),
            Cmd::NewTab => t!("cmd.kw.new_tab"),
            Cmd::CloseTab => t!("cmd.kw.close_tab"),
            Cmd::SwitchPane => t!("cmd.kw.switch_pane"),
            Cmd::SplitPane => t!("cmd.kw.split_pane"),
            Cmd::ClosePane => t!("cmd.kw.close_pane"),
            Cmd::FocusPaneLeft | Cmd::FocusPaneRight => t!("cmd.kw.focus_pane"),
            Cmd::EditPath => t!("cmd.kw.edit_path"),
            Cmd::GoToPath => t!("cmd.kw.go_to_path"),
            Cmd::Filter => t!("cmd.kw.filter"),
            Cmd::AddToFavorites => t!("cmd.kw.add_to_favorites"),
            Cmd::Connect => t!("cmd.kw.connect"),
            Cmd::Transfers => t!("cmd.kw.transfers"),
            Cmd::CompareSync => t!("cmd.kw.compare_sync"),
            Cmd::CommandPalette => t!("cmd.kw.command_palette"),
            Cmd::Settings => t!("cmd.kw.settings"),
            Cmd::Help => t!("cmd.kw.help"),
            Cmd::Support => t!("cmd.kw.support"),
            Cmd::SourceCode => t!("cmd.kw.source_code"),
            Cmd::ReportIssue => t!("cmd.kw.report_issue"),
            Cmd::SuggestIdea => t!("cmd.kw.suggest_idea"),
            Cmd::ThirdPartyLicenses => t!("cmd.kw.third_party_licenses"),
            Cmd::ReloadPlugins => t!("cmd.kw.reload_plugins"),
            Cmd::BrowsePlugins => t!("cmd.kw.browse_plugins"),
            Cmd::Compress => t!("cmd.kw.compress"),
            Cmd::ExtractHere => t!("cmd.kw.extract_here"),
            Cmd::OpenWithApp => t!("cmd.kw.open_with_app"),
            Cmd::OpenShellInContainer => t!("cmd.kw.open_shell_in_container"),
            Cmd::AppearanceLight | Cmd::AppearanceDark | Cmd::AppearanceSystem => {
                t!("cmd.kw.appearance")
            }
            Cmd::ViewIcons | Cmd::ViewList | Cmd::ViewColumns | Cmd::ViewGallery => {
                t!("cmd.kw.view")
            }
        }
    }
}

/// Score `query` against a command: `None` if it doesn't match. Higher is
/// better. Matches subsequences of the label (word starts count more), or
/// any keyword substring.
#[cfg(test)]
pub fn match_score(cmd: Cmd, query: &str) -> Option<i32> {
    match_text(cmd.label(), cmd.keywords(), cmd.category(), query)
}

/// [`match_score`] for any palette entry (plugin commands too).
pub fn match_text(label: &str, keywords: &str, category: &str, query: &str) -> Option<i32> {
    let q = query.trim().to_lowercase();
    if q.is_empty() {
        return Some(0);
    }
    let label = label.to_lowercase();
    if label.starts_with(&q) {
        return Some(1000 - label.len() as i32);
    }
    if let Some(pos) = label.find(&q) {
        return Some(800 - pos as i32);
    }
    // Subsequence with word-start bonus.
    let chars: Vec<char> = label.chars().collect();
    let mut score = 0;
    let mut li = 0;
    for qc in q.chars().filter(|c| !c.is_whitespace()) {
        let mut found = false;
        while li < chars.len() {
            let c = chars[li];
            let word_start = li == 0 || !chars[li - 1].is_alphanumeric();
            li += 1;
            if c == qc {
                score += if word_start { 30 } else { 5 };
                found = true;
                break;
            }
        }
        if !found {
            score = -1;
            break;
        }
    }
    if score > 0 {
        return Some(score);
    }
    let category = category.to_lowercase();
    if keywords.split(' ').any(|w| w.starts_with(&q)) || category.starts_with(&q) {
        return Some(100);
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn palette_matching() {
        assert!(match_score(Cmd::NewFolder, "new f").is_some());
        assert!(match_score(Cmd::NewFolder, "nf").is_some());
        assert!(match_score(Cmd::Connect, "sftp").is_some());
        assert!(match_score(Cmd::MoveToTrash, "xyz").is_none());
        let best = ALL
            .iter()
            .copied()
            .max_by_key(|c| match_score(*c, "ren").unwrap_or(-1))
            .unwrap();
        assert_eq!(best, Cmd::Rename);
    }

    /// Finder's shortcuts for the same actions (see docs/manual, shortcuts).
    #[test]
    fn finder_shortcuts() {
        assert_eq!(Cmd::NewTab.shortcut(), "⌘T");
        assert_eq!(Cmd::MoveToTrash.shortcut(), "⌘⌫");
        assert_eq!(Cmd::DeletePermanently.shortcut(), "⌥⌘⌫");
        assert_eq!(Cmd::Transfers.shortcut(), "⌥⌘L");
        assert_eq!(Cmd::NewFolder.shortcut(), "⇧⌘N");
        assert_eq!(Cmd::GoToPath.shortcut(), "⇧⌘G");
        assert_eq!(Cmd::ToggleHidden.shortcut(), "⇧⌘.");
        assert_eq!(Cmd::AddToFavorites.shortcut(), "⌃⌘T");
        assert_eq!(Cmd::EmptyTrash.shortcut(), "⇧⌘⌫");
    }

    /// "trash" finds Move to Trash, "permanent" only the permanent delete.
    #[test]
    fn trash_keywords_match_what_the_commands_do() {
        crate::i18n::with_locale(crate::i18n::Locale::En, || {
            assert!(match_score(Cmd::MoveToTrash, "trash").is_some());
            assert!(match_score(Cmd::DeletePermanently, "permanent").is_some());
            assert!(match_score(Cmd::DeletePermanently, "trash").is_none());
        });
        crate::i18n::with_locale(crate::i18n::Locale::De, || {
            assert!(match_score(Cmd::MoveToTrash, "papierkorb").is_some());
            assert!(match_score(Cmd::DeletePermanently, "endgültig").is_some());
            assert!(match_score(Cmd::DeletePermanently, "papierkorb").is_none());
        });
    }

    /// "support" / "kontakt" find the website link in both languages.
    #[test]
    fn support_keywords() {
        for (loc, words) in [
            (crate::i18n::Locale::En, ["support", "website", "contact"]),
            (crate::i18n::Locale::De, ["support", "webseite", "kontakt"]),
        ] {
            crate::i18n::with_locale(loc, || {
                for w in words {
                    let best = ALL
                        .iter()
                        .copied()
                        .max_by_key(|c| match_score(*c, w).unwrap_or(-1))
                        .unwrap();
                    assert_eq!(best, Cmd::Support, "{w}");
                }
            });
        }
    }

    #[test]
    fn shortcuts_are_unique() {
        let mut seen = std::collections::HashSet::new();
        for c in ALL {
            assert!(
                c.shortcut().is_empty() || seen.insert(c.shortcut()),
                "duplicate shortcut {}",
                c.shortcut()
            );
        }
    }
}
