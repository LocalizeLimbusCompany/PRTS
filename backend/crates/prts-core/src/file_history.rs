//! 文件/文件夹操作、恢复与回滚的领域真值。
//!
//! 本模块接收已经锁定并物化的树、文件与词条快照，输出数据库适配器可直接执行的
//! typed plan。路径环、active path 冲突、删除 operation ownership、恢复 exposure、
//! rollback current→target 差异和 0 CP 均在这里决定；SQL 与 axum handler 不得再定义
//! 第二套规则。

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::ops::{AddAssign, Neg};

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::upload_replacement::EntryHistorySnapshot;
use crate::EntryState;

/// 文件/文件夹软删除的默认保留天数。
pub const DEFAULT_RETENTION_DAYS: i64 = 30;

/// entry 历史 JSON 的唯一允许字段。
pub const ENTRY_HISTORY_FIELDS: &[&str] = &[
    "key",
    "original",
    "translation",
    "state",
    "locked",
    "hidden",
    "questioned",
    "deleted_at",
];

/// file 历史 JSON 的唯一允许字段。
pub const FILE_HISTORY_FIELDS: &[&str] = &["folder_id", "name", "path", "deleted_at"];

/// folder 历史 JSON 的唯一允许字段。
pub const FOLDER_HISTORY_FIELDS: &[&str] = &["parent_id", "name", "path", "deleted_at"];

/// change set 的领域操作类型。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileHistoryOperation {
    /// 移动到另一个父文件夹。
    Move,
    /// 在同一父文件夹内重命名。
    Rename,
    /// 创建可恢复软删除。
    Delete,
    /// 恢复某个明确删除 operation 持有的行。
    Restore,
    /// 从历史目标物化出新版本。
    Rollback,
}

/// change set 的业务目标。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileHistoryTarget {
    /// 文件目标。
    File(i64),
    /// 文件夹目标。
    Folder(i64),
}

/// `file_change_items.entity_type` 的领域值。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FileHistoryEntity {
    /// 文件夹。
    Folder,
    /// 文件。
    File,
    /// 词条。
    Entry,
}

/// `file_change_items.operation` 的领域值。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileHistoryItemOperation {
    /// 常规字段更新。
    Update,
    /// 结构路径移动。
    Move,
    /// 软删除。
    Delete,
    /// 恢复。
    Restore,
    /// 词条 tombstone。
    Tombstone,
}

/// 文件内物化的 effective-visible 状态计数。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct MaterializedFileStats {
    /// 可见词条总数。
    pub visible_total: i64,
    /// 未翻译数。
    pub untranslated: i64,
    /// 已翻译数。
    pub translated: i64,
    /// 有疑问数。
    pub questioned: i64,
    /// 已检查数。
    pub checked: i64,
    /// 已审核数。
    pub reviewed: i64,
    /// 隐藏但未 tombstone 的词条总数。
    pub hidden_total: i64,
    /// 隐藏未翻译数。
    pub hidden_untranslated: i64,
    /// 隐藏已翻译数。
    pub hidden_translated: i64,
    /// 隐藏有疑问数。
    pub hidden_questioned: i64,
    /// 隐藏已检查数。
    pub hidden_checked: i64,
    /// 隐藏已审核数。
    pub hidden_reviewed: i64,
}

impl AddAssign for MaterializedFileStats {
    fn add_assign(&mut self, rhs: Self) {
        self.visible_total += rhs.visible_total;
        self.untranslated += rhs.untranslated;
        self.translated += rhs.translated;
        self.questioned += rhs.questioned;
        self.checked += rhs.checked;
        self.reviewed += rhs.reviewed;
        self.hidden_total += rhs.hidden_total;
        self.hidden_untranslated += rhs.hidden_untranslated;
        self.hidden_translated += rhs.hidden_translated;
        self.hidden_questioned += rhs.hidden_questioned;
        self.hidden_checked += rhs.hidden_checked;
        self.hidden_reviewed += rhs.hidden_reviewed;
    }
}

impl Neg for MaterializedFileStats {
    type Output = Self;

    fn neg(self) -> Self::Output {
        Self {
            visible_total: -self.visible_total,
            untranslated: -self.untranslated,
            translated: -self.translated,
            questioned: -self.questioned,
            checked: -self.checked,
            reviewed: -self.reviewed,
            hidden_total: -self.hidden_total,
            hidden_untranslated: -self.hidden_untranslated,
            hidden_translated: -self.hidden_translated,
            hidden_questioned: -self.hidden_questioned,
            hidden_checked: -self.hidden_checked,
            hidden_reviewed: -self.hidden_reviewed,
        }
    }
}

impl MaterializedFileStats {
    fn add_entry(&mut self, entry: &EntryHistorySnapshot, amount: i64) {
        if entry.deleted {
            return;
        }
        if entry.hidden {
            self.hidden_total += amount;
            match entry.state {
                EntryState::Untranslated => self.hidden_untranslated += amount,
                EntryState::Translated => self.hidden_translated += amount,
                EntryState::Checked => self.hidden_checked += amount,
                EntryState::Reviewed => self.hidden_reviewed += amount,
            }
            if entry.questioned {
                self.hidden_questioned += amount;
            }
        } else {
            self.visible_total += amount;
            match entry.state {
                EntryState::Untranslated => self.untranslated += amount,
                EntryState::Translated => self.translated += amount,
                EntryState::Checked => self.checked += amount,
                EntryState::Reviewed => self.reviewed += amount,
            }
            if entry.questioned {
                self.questioned += amount;
            }
        }
    }

    fn between_entries(before: &EntryHistorySnapshot, after: &EntryHistorySnapshot) -> Self {
        let mut delta = Self::default();
        delta.add_entry(before, -1);
        delta.add_entry(after, 1);
        delta
    }
}

/// 锁定事务中物化的文件夹行。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FolderNode {
    /// 数据库 id。
    pub id: i64,
    /// 父文件夹；根级为 `None`。
    pub parent_id: Option<i64>,
    /// 当前名称。
    pub name: String,
    /// 当前规范化路径。
    pub path: String,
    /// `None` 表示 active；`Some` 表示该删除 operation 持有此行。
    pub deletion_operation_id: Option<Uuid>,
}

impl FolderNode {
    /// 行当前是否 active。
    pub fn is_active(&self) -> bool {
        self.deletion_operation_id.is_none()
    }

    fn history_snapshot(&self) -> FolderHistorySnapshot {
        FolderHistorySnapshot {
            parent_id: self.parent_id,
            name: self.name.clone(),
            path: self.path.clone(),
            deleted: !self.is_active(),
        }
    }
}

/// 锁定事务中物化的文件行及其 file_stats。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileNode {
    /// 数据库 id。
    pub id: i64,
    /// 所属文件夹；根级为 `None`。
    pub folder_id: Option<i64>,
    /// 当前名称。
    pub name: String,
    /// 当前规范化路径。
    pub path: String,
    /// `None` 表示 active；`Some` 表示该删除 operation 持有此行。
    pub deletion_operation_id: Option<Uuid>,
    /// 文件内 active、非 hidden entry 的物化计数；文件删除时仍保留。
    pub stats: MaterializedFileStats,
}

impl FileNode {
    /// 行当前是否 active。
    pub fn is_active(&self) -> bool {
        self.deletion_operation_id.is_none()
    }

    fn history_snapshot(&self) -> FileStructureSnapshot {
        FileStructureSnapshot {
            folder_id: self.folder_id,
            name: self.name.clone(),
            path: self.path.clone(),
            deleted: !self.is_active(),
        }
    }
}

/// folder before/after 的领域 allowlist 快照。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FolderHistorySnapshot {
    /// 父文件夹。
    pub parent_id: Option<i64>,
    /// 名称。
    pub name: String,
    /// 规范化路径。
    pub path: String,
    /// adapter 必须显式映射为 `deleted_at` 的有无。
    #[serde(skip)]
    pub deleted: bool,
}

/// file before/after 的领域 allowlist 快照。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FileStructureSnapshot {
    /// 所属文件夹。
    pub folder_id: Option<i64>,
    /// 名称。
    pub name: String,
    /// 规范化路径。
    pub path: String,
    /// adapter 必须显式映射为 `deleted_at` 的有无。
    #[serde(skip)]
    pub deleted: bool,
}

/// 历史 before/after 的封闭 typed union。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FileHistorySnapshot {
    /// 文件夹快照。
    Folder(FolderHistorySnapshot),
    /// 文件快照。
    File(FileStructureSnapshot),
    /// 词条快照。
    Entry(EntryHistorySnapshot),
}

/// 一条明确的 allowlisted history delta。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileHistoryDelta {
    /// 业务实体类型。
    pub entity: FileHistoryEntity,
    /// 永久实体 id snapshot。
    pub entity_id: i64,
    /// item 操作类型。
    pub operation: FileHistoryItemOperation,
    /// 变更前快照。
    pub before: Option<FileHistorySnapshot>,
    /// 变更后快照。
    pub after: Option<FileHistorySnapshot>,
    /// change set 内稳定顺序。
    pub ordinal: i32,
}

/// 数据库 adapter 必须执行的 typed mutation。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FileHistoryMutation {
    /// 更新文件夹 parent/name/path；删除 ownership 不变。
    UpdateFolderStructure {
        /// 变更前状态。
        before: FolderNode,
        /// 变更后状态。
        after: FolderNode,
    },
    /// 更新文件 folder/name/path；删除 ownership 不变。
    UpdateFileStructure {
        /// 变更前状态。
        before: FileNode,
        /// 变更后状态。
        after: FileNode,
    },
    /// 用本 change set 标记 active 文件夹。
    DeleteFolder {
        /// 变更前 active 行。
        folder: FolderNode,
        /// deletion_change_set_id。
        operation_id: Uuid,
    },
    /// 用本 change set 标记 active 文件。
    DeleteFile {
        /// 变更前 active 行。
        file: FileNode,
        /// deletion_change_set_id。
        operation_id: Uuid,
    },
    /// 只清除匹配 source operation 的文件夹删除字段。
    RestoreFolder {
        /// 变更前 deleted 行。
        folder: FolderNode,
        /// 必须匹配的 deletion_change_set_id。
        source_operation_id: Uuid,
    },
    /// 只清除匹配 source operation 的文件删除字段。
    RestoreFile {
        /// 变更前 deleted 行。
        file: FileNode,
        /// 必须匹配的 deletion_change_set_id。
        source_operation_id: Uuid,
    },
    /// rollback 写入词条目标状态并递增版本。
    ReplaceEntry {
        /// 永久词条 id。
        entry_id: i64,
        /// 当前状态。
        before: EntryHistorySnapshot,
        /// rollback 目标状态。
        after: EntryHistorySnapshot,
    },
}

/// prts-db 可直接执行的一份文件历史计划。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileHistoryPlan {
    /// 本次新 change set id。
    pub change_set_id: Uuid,
    /// 本次操作类型。
    pub operation: FileHistoryOperation,
    /// 本次业务目标。
    pub target: FileHistoryTarget,
    /// 变更发生时的安全路径快照。
    pub path_snapshot: String,
    /// restore 的原删除 operation，或 rollback 的目标历史 change set。
    pub source_change_set_id: Option<Uuid>,
    /// adapter 必须逐项执行的 mutation。
    pub mutations: Vec<FileHistoryMutation>,
    /// adapter 必须原样持久化的 typed history delta。
    pub history: Vec<FileHistoryDelta>,
    /// 对 project/task exposure 的物化 file stats 差异。
    pub project_stats_delta: MaterializedFileStats,
    /// 词条 rollback 同时需要修改目标 file_stats；结构删除/恢复必须保持 file_stats。
    pub file_stats_delta: Option<(i64, MaterializedFileStats)>,
    /// 恢复与回滚固定为 0；本阶段其它文件操作也不产生 CP。
    pub cp_delta_tenths: i64,
}

/// active 路径冲突检查输入。folder/file 各自遵循数据库的部分唯一索引。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ActivePathIndex {
    /// active folder paths。
    pub folders: BTreeSet<String>,
    /// active file paths。
    pub files: BTreeSet<String>,
}

/// rollback 物化版本中的词条状态。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VersionedEntry {
    /// 永久词条 id。
    pub id: i64,
    /// allowlisted 状态。
    pub snapshot: EntryHistorySnapshot,
}

/// rollback 的当前或目标文件版本。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MaterializedFileVersion {
    /// file 结构与当前 deletion ownership。
    pub file: FileNode,
    /// 以永久 id 标识的完整词条集合；deleted tombstone 也必须包含。
    pub entries: Vec<VersionedEntry>,
}

/// typed plan 无法安全生成时的稳定领域错误。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FileHistoryPlanError {
    /// 名称为空、含路径分隔符或为保留段。
    InvalidName,
    /// 操作目标不是 active 行。
    TargetDeleted,
    /// destination parent 已删除。
    DestinationDeleted,
    /// 文件仍位于 deleted ancestor 下，必须先恢复拥有该树的 folder operation。
    AncestorDeleted,
    /// 文件夹被移入自身或后代。
    MoveIntoDescendant,
    /// 新路径与现有 active 行冲突。
    PathConflict { path: String },
    /// 请求没有产生结构变化。
    NoChange,
    /// 树输入缺少根目标。
    MissingTarget,
    /// 树输入出现重复 id。
    DuplicateEntity { id: i64 },
    /// 恢复请求不拥有目标删除 operation。
    OperationNotOwned {
        expected: Uuid,
        actual: Option<Uuid>,
    },
    /// folder parent 链缺少节点或形成环。
    InvalidTree,
    /// rollback 的 current/target 不是同一文件。
    RollbackTargetMismatch,
    /// 目标历史引用的词条已不在当前可恢复集合中。
    MissingRollbackEntry { entry_id: i64 },
    /// rollback 目标文件本身为 deleted；应使用 restore。
    RollbackTargetDeleted,
}

/// 为 active 文件生成 move/rename plan。
pub fn plan_file_move(
    change_set_id: Uuid,
    file: FileNode,
    destination: Option<&FolderNode>,
    new_name: &str,
    active_paths: &ActivePathIndex,
) -> Result<FileHistoryPlan, FileHistoryPlanError> {
    validate_name(new_name)?;
    if !file.is_active() {
        return Err(FileHistoryPlanError::TargetDeleted);
    }
    if destination.is_some_and(|folder| !folder.is_active()) {
        return Err(FileHistoryPlanError::DestinationDeleted);
    }
    let destination_folder_id = destination.map(|folder| folder.id);
    let new_path = join_path(destination.map(|folder| folder.path.as_str()), new_name);
    if new_path == file.path && destination_folder_id == file.folder_id && new_name == file.name {
        return Err(FileHistoryPlanError::NoChange);
    }
    if active_paths.files.contains(&new_path) && new_path != file.path {
        return Err(FileHistoryPlanError::PathConflict { path: new_path });
    }

    let operation = if destination_folder_id == file.folder_id {
        FileHistoryOperation::Rename
    } else {
        FileHistoryOperation::Move
    };
    let mut after = file.clone();
    after.folder_id = destination_folder_id;
    after.name = new_name.to_string();
    after.path = new_path;
    let history_operation = if operation == FileHistoryOperation::Rename {
        FileHistoryItemOperation::Update
    } else {
        FileHistoryItemOperation::Move
    };
    Ok(FileHistoryPlan {
        change_set_id,
        operation,
        target: FileHistoryTarget::File(file.id),
        path_snapshot: file.path.clone(),
        source_change_set_id: None,
        mutations: vec![FileHistoryMutation::UpdateFileStructure {
            before: file.clone(),
            after: after.clone(),
        }],
        history: vec![FileHistoryDelta {
            entity: FileHistoryEntity::File,
            entity_id: file.id,
            operation: history_operation,
            before: Some(FileHistorySnapshot::File(file.history_snapshot())),
            after: Some(FileHistorySnapshot::File(after.history_snapshot())),
            ordinal: 0,
        }],
        project_stats_delta: MaterializedFileStats::default(),
        file_stats_delta: None,
        cp_delta_tenths: 0,
    })
}

/// 为文件夹根及其全部后代生成 move/rename plan。
pub fn plan_folder_move(
    change_set_id: Uuid,
    root: FolderNode,
    descendant_folders: Vec<FolderNode>,
    descendant_files: Vec<FileNode>,
    destination: Option<&FolderNode>,
    new_name: &str,
    active_paths: &ActivePathIndex,
) -> Result<FileHistoryPlan, FileHistoryPlanError> {
    validate_name(new_name)?;
    if !root.is_active() {
        return Err(FileHistoryPlanError::TargetDeleted);
    }
    if destination.is_some_and(|folder| !folder.is_active()) {
        return Err(FileHistoryPlanError::DestinationDeleted);
    }
    if destination.is_some_and(|folder| path_is_self_or_descendant(&folder.path, &root.path)) {
        return Err(FileHistoryPlanError::MoveIntoDescendant);
    }

    let destination_parent_id = destination.map(|folder| folder.id);
    let new_root_path = join_path(destination.map(|folder| folder.path.as_str()), new_name);
    if new_root_path == root.path
        && destination_parent_id == root.parent_id
        && new_name == root.name
    {
        return Err(FileHistoryPlanError::NoChange);
    }

    validate_unique_ids(
        std::iter::once(root.id).chain(descendant_folders.iter().map(|folder| folder.id)),
    )?;
    validate_unique_ids(descendant_files.iter().map(|file| file.id))?;
    if descendant_folders
        .iter()
        .any(|folder| !path_is_descendant(&folder.path, &root.path))
        || descendant_files
            .iter()
            .any(|file| !path_is_descendant(&file.path, &root.path))
    {
        return Err(FileHistoryPlanError::InvalidTree);
    }

    let moving_folder_paths = std::iter::once(root.path.clone())
        .chain(descendant_folders.iter().map(|folder| folder.path.clone()))
        .collect::<HashSet<_>>();
    let moving_file_paths = descendant_files
        .iter()
        .map(|file| file.path.clone())
        .collect::<HashSet<_>>();
    let mut changes = Vec::with_capacity(1 + descendant_folders.len() + descendant_files.len());

    let mut root_after = root.clone();
    root_after.parent_id = destination_parent_id;
    root_after.name = new_name.to_string();
    root_after.path = new_root_path.clone();
    changes.push(StructureChange::Folder(root.clone(), root_after));
    for folder in descendant_folders {
        let mut after = folder.clone();
        after.path = replace_path_prefix(&folder.path, &root.path, &new_root_path)?;
        changes.push(StructureChange::Folder(folder, after));
    }
    for file in descendant_files {
        let mut after = file.clone();
        after.path = replace_path_prefix(&file.path, &root.path, &new_root_path)?;
        changes.push(StructureChange::File(file, after));
    }

    for change in &changes {
        match change {
            StructureChange::Folder(_, after) => {
                if active_paths.folders.contains(&after.path)
                    && !moving_folder_paths.contains(&after.path)
                {
                    return Err(FileHistoryPlanError::PathConflict {
                        path: after.path.clone(),
                    });
                }
            }
            StructureChange::File(_, after) => {
                if active_paths.files.contains(&after.path)
                    && !moving_file_paths.contains(&after.path)
                {
                    return Err(FileHistoryPlanError::PathConflict {
                        path: after.path.clone(),
                    });
                }
            }
        }
    }

    let operation = if destination_parent_id == root.parent_id {
        FileHistoryOperation::Rename
    } else {
        FileHistoryOperation::Move
    };
    let mut mutations = Vec::with_capacity(changes.len());
    let mut history = Vec::with_capacity(changes.len());
    for (ordinal, change) in changes.into_iter().enumerate() {
        let ordinal = i32::try_from(ordinal).map_err(|_| FileHistoryPlanError::InvalidTree)?;
        match change {
            StructureChange::Folder(before, after) => {
                history.push(FileHistoryDelta {
                    entity: FileHistoryEntity::Folder,
                    entity_id: before.id,
                    operation: FileHistoryItemOperation::Move,
                    before: Some(FileHistorySnapshot::Folder(before.history_snapshot())),
                    after: Some(FileHistorySnapshot::Folder(after.history_snapshot())),
                    ordinal,
                });
                mutations.push(FileHistoryMutation::UpdateFolderStructure { before, after });
            }
            StructureChange::File(before, after) => {
                history.push(FileHistoryDelta {
                    entity: FileHistoryEntity::File,
                    entity_id: before.id,
                    operation: FileHistoryItemOperation::Move,
                    before: Some(FileHistorySnapshot::File(before.history_snapshot())),
                    after: Some(FileHistorySnapshot::File(after.history_snapshot())),
                    ordinal,
                });
                mutations.push(FileHistoryMutation::UpdateFileStructure { before, after });
            }
        }
    }

    Ok(FileHistoryPlan {
        change_set_id,
        operation,
        target: FileHistoryTarget::Folder(root.id),
        path_snapshot: root.path,
        source_change_set_id: None,
        mutations,
        history,
        project_stats_delta: MaterializedFileStats::default(),
        file_stats_delta: None,
        cp_delta_tenths: 0,
    })
}

/// 为单个 active 文件生成可恢复删除 plan。
pub fn plan_file_delete(
    change_set_id: Uuid,
    file: FileNode,
) -> Result<FileHistoryPlan, FileHistoryPlanError> {
    if !file.is_active() {
        return Err(FileHistoryPlanError::TargetDeleted);
    }
    let mut after = file.clone();
    after.deletion_operation_id = Some(change_set_id);
    Ok(FileHistoryPlan {
        change_set_id,
        operation: FileHistoryOperation::Delete,
        target: FileHistoryTarget::File(file.id),
        path_snapshot: file.path.clone(),
        source_change_set_id: None,
        mutations: vec![FileHistoryMutation::DeleteFile {
            file: file.clone(),
            operation_id: change_set_id,
        }],
        history: vec![FileHistoryDelta {
            entity: FileHistoryEntity::File,
            entity_id: file.id,
            operation: FileHistoryItemOperation::Delete,
            before: Some(FileHistorySnapshot::File(file.history_snapshot())),
            after: Some(FileHistorySnapshot::File(after.history_snapshot())),
            ordinal: 0,
        }],
        project_stats_delta: -file.stats,
        file_stats_delta: None,
        cp_delta_tenths: 0,
    })
}

/// 为 active 文件夹根生成删除 plan；只标记此前 active 的 subtree rows。
pub fn plan_folder_delete(
    change_set_id: Uuid,
    root_id: i64,
    folders: Vec<FolderNode>,
    files: Vec<FileNode>,
) -> Result<FileHistoryPlan, FileHistoryPlanError> {
    validate_unique_ids(folders.iter().map(|folder| folder.id))?;
    validate_unique_ids(files.iter().map(|file| file.id))?;
    let root = folders
        .iter()
        .find(|folder| folder.id == root_id)
        .cloned()
        .ok_or(FileHistoryPlanError::MissingTarget)?;
    if !root.is_active() {
        return Err(FileHistoryPlanError::TargetDeleted);
    }
    if folders
        .iter()
        .any(|folder| folder.id != root_id && !path_is_descendant(&folder.path, &root.path))
        || files
            .iter()
            .any(|file| !path_is_descendant(&file.path, &root.path))
    {
        return Err(FileHistoryPlanError::InvalidTree);
    }

    let mut active_folders = folders
        .into_iter()
        .filter(FolderNode::is_active)
        .collect::<Vec<_>>();
    let mut active_files = files
        .into_iter()
        .filter(FileNode::is_active)
        .collect::<Vec<_>>();
    active_folders.sort_by(|left, right| left.path.cmp(&right.path));
    active_files.sort_by(|left, right| left.path.cmp(&right.path));

    let mut mutations = Vec::with_capacity(active_folders.len() + active_files.len());
    let mut history = Vec::with_capacity(active_folders.len() + active_files.len());
    let mut stats_delta = MaterializedFileStats::default();
    for folder in active_folders {
        let mut after = folder.clone();
        after.deletion_operation_id = Some(change_set_id);
        let ordinal =
            i32::try_from(history.len()).map_err(|_| FileHistoryPlanError::InvalidTree)?;
        history.push(FileHistoryDelta {
            entity: FileHistoryEntity::Folder,
            entity_id: folder.id,
            operation: FileHistoryItemOperation::Delete,
            before: Some(FileHistorySnapshot::Folder(folder.history_snapshot())),
            after: Some(FileHistorySnapshot::Folder(after.history_snapshot())),
            ordinal,
        });
        mutations.push(FileHistoryMutation::DeleteFolder {
            folder,
            operation_id: change_set_id,
        });
    }
    for file in active_files {
        let mut after = file.clone();
        after.deletion_operation_id = Some(change_set_id);
        let ordinal =
            i32::try_from(history.len()).map_err(|_| FileHistoryPlanError::InvalidTree)?;
        stats_delta += -file.stats;
        history.push(FileHistoryDelta {
            entity: FileHistoryEntity::File,
            entity_id: file.id,
            operation: FileHistoryItemOperation::Delete,
            before: Some(FileHistorySnapshot::File(file.history_snapshot())),
            after: Some(FileHistorySnapshot::File(after.history_snapshot())),
            ordinal,
        });
        mutations.push(FileHistoryMutation::DeleteFile {
            file,
            operation_id: change_set_id,
        });
    }

    Ok(FileHistoryPlan {
        change_set_id,
        operation: FileHistoryOperation::Delete,
        target: FileHistoryTarget::Folder(root.id),
        path_snapshot: root.path,
        source_change_set_id: None,
        mutations,
        history,
        project_stats_delta: stats_delta,
        file_stats_delta: None,
        cp_delta_tenths: 0,
    })
}

/// 恢复一个被明确 delete operation 持有的文件。
pub fn plan_file_restore(
    change_set_id: Uuid,
    source_operation_id: Uuid,
    file: FileNode,
    all_folders: &[FolderNode],
    active_paths: &ActivePathIndex,
) -> Result<FileHistoryPlan, FileHistoryPlanError> {
    require_operation_owner(file.deletion_operation_id, source_operation_id)?;
    if active_paths.files.contains(&file.path) {
        return Err(FileHistoryPlanError::PathConflict {
            path: file.path.clone(),
        });
    }
    let folder_map = folder_map(all_folders)?;
    let exposed_after = file
        .folder_id
        .map(|folder_id| folder_chain_is_active(folder_id, &folder_map))
        .transpose()?
        .unwrap_or(true);
    if !exposed_after {
        return Err(FileHistoryPlanError::AncestorDeleted);
    }
    let mut after = file.clone();
    after.deletion_operation_id = None;
    Ok(FileHistoryPlan {
        change_set_id,
        operation: FileHistoryOperation::Restore,
        target: FileHistoryTarget::File(file.id),
        path_snapshot: file.path.clone(),
        source_change_set_id: Some(source_operation_id),
        mutations: vec![FileHistoryMutation::RestoreFile {
            file: file.clone(),
            source_operation_id,
        }],
        history: vec![FileHistoryDelta {
            entity: FileHistoryEntity::File,
            entity_id: file.id,
            operation: FileHistoryItemOperation::Restore,
            before: Some(FileHistorySnapshot::File(file.history_snapshot())),
            after: Some(FileHistorySnapshot::File(after.history_snapshot())),
            ordinal: 0,
        }],
        project_stats_delta: file.stats,
        file_stats_delta: None,
        cp_delta_tenths: 0,
    })
}

/// 恢复文件夹删除 operation；只清除匹配 operation id 的 rows。
pub fn plan_folder_restore(
    change_set_id: Uuid,
    source_operation_id: Uuid,
    root_id: i64,
    all_folders: Vec<FolderNode>,
    subtree_files: Vec<FileNode>,
    active_paths: &ActivePathIndex,
) -> Result<FileHistoryPlan, FileHistoryPlanError> {
    validate_unique_ids(all_folders.iter().map(|folder| folder.id))?;
    validate_unique_ids(subtree_files.iter().map(|file| file.id))?;
    let root = all_folders
        .iter()
        .find(|folder| folder.id == root_id)
        .cloned()
        .ok_or(FileHistoryPlanError::MissingTarget)?;
    require_operation_owner(root.deletion_operation_id, source_operation_id)?;
    if subtree_files
        .iter()
        .any(|file| !path_is_descendant(&file.path, &root.path))
    {
        return Err(FileHistoryPlanError::InvalidTree);
    }
    let folder_map = folder_map(&all_folders)?;
    let mut matching_folders = all_folders
        .into_iter()
        .filter(|folder| {
            path_is_self_or_descendant(&folder.path, &root.path)
                && folder.deletion_operation_id == Some(source_operation_id)
        })
        .collect::<Vec<_>>();
    let mut matching_files = subtree_files
        .into_iter()
        .filter(|file| file.deletion_operation_id == Some(source_operation_id))
        .collect::<Vec<_>>();
    if let Some(path) = matching_folders
        .iter()
        .map(|folder| &folder.path)
        .find(|path| active_paths.folders.contains(*path))
        .or_else(|| {
            matching_files
                .iter()
                .map(|file| &file.path)
                .find(|path| active_paths.files.contains(*path))
        })
    {
        return Err(FileHistoryPlanError::PathConflict { path: path.clone() });
    }
    matching_folders.sort_by(|left, right| left.path.cmp(&right.path));
    matching_files.sort_by(|left, right| left.path.cmp(&right.path));

    let mut mutations = Vec::with_capacity(matching_folders.len() + matching_files.len());
    let mut history = Vec::with_capacity(matching_folders.len() + matching_files.len());
    let mut stats_delta = MaterializedFileStats::default();
    for folder in matching_folders {
        let mut after = folder.clone();
        after.deletion_operation_id = None;
        let ordinal =
            i32::try_from(history.len()).map_err(|_| FileHistoryPlanError::InvalidTree)?;
        history.push(FileHistoryDelta {
            entity: FileHistoryEntity::Folder,
            entity_id: folder.id,
            operation: FileHistoryItemOperation::Restore,
            before: Some(FileHistorySnapshot::Folder(folder.history_snapshot())),
            after: Some(FileHistorySnapshot::Folder(after.history_snapshot())),
            ordinal,
        });
        mutations.push(FileHistoryMutation::RestoreFolder {
            folder,
            source_operation_id,
        });
    }
    for file in matching_files {
        let exposed_after = file
            .folder_id
            .map(|folder_id| folder_chain_active_after(folder_id, &folder_map, source_operation_id))
            .transpose()?
            .unwrap_or(true);
        if exposed_after {
            stats_delta += file.stats;
        }
        let mut after = file.clone();
        after.deletion_operation_id = None;
        let ordinal =
            i32::try_from(history.len()).map_err(|_| FileHistoryPlanError::InvalidTree)?;
        history.push(FileHistoryDelta {
            entity: FileHistoryEntity::File,
            entity_id: file.id,
            operation: FileHistoryItemOperation::Restore,
            before: Some(FileHistorySnapshot::File(file.history_snapshot())),
            after: Some(FileHistorySnapshot::File(after.history_snapshot())),
            ordinal,
        });
        mutations.push(FileHistoryMutation::RestoreFile {
            file,
            source_operation_id,
        });
    }

    Ok(FileHistoryPlan {
        change_set_id,
        operation: FileHistoryOperation::Restore,
        target: FileHistoryTarget::Folder(root.id),
        path_snapshot: root.path,
        source_change_set_id: Some(source_operation_id),
        mutations,
        history,
        project_stats_delta: stats_delta,
        file_stats_delta: None,
        cp_delta_tenths: 0,
    })
}

/// 从服务端物化的 target version 生成 current→target 新 rollback delta。
pub fn plan_file_rollback(
    change_set_id: Uuid,
    target_change_set_id: Uuid,
    current: MaterializedFileVersion,
    target: MaterializedFileVersion,
    active_paths: &ActivePathIndex,
) -> Result<FileHistoryPlan, FileHistoryPlanError> {
    if current.file.id != target.file.id {
        return Err(FileHistoryPlanError::RollbackTargetMismatch);
    }
    if !current.file.is_active() || !target.file.is_active() {
        return Err(FileHistoryPlanError::RollbackTargetDeleted);
    }
    if current.file.path != target.file.path
        && active_paths.files.contains(&target.file.path)
        && target.file.path != current.file.path
    {
        return Err(FileHistoryPlanError::PathConflict {
            path: target.file.path,
        });
    }
    validate_unique_ids(current.entries.iter().map(|entry| entry.id))?;
    validate_unique_ids(target.entries.iter().map(|entry| entry.id))?;

    let current_by_id = current
        .entries
        .into_iter()
        .map(|entry| (entry.id, entry.snapshot))
        .collect::<BTreeMap<_, _>>();
    let target_by_id = target
        .entries
        .into_iter()
        .map(|entry| (entry.id, entry.snapshot))
        .collect::<BTreeMap<_, _>>();

    for entry_id in target_by_id.keys() {
        if !current_by_id.contains_key(entry_id) {
            return Err(FileHistoryPlanError::MissingRollbackEntry {
                entry_id: *entry_id,
            });
        }
    }

    let path_snapshot = current.file.path.clone();
    let file_id = current.file.id;
    let mut mutations = Vec::new();
    let mut history = Vec::new();
    let structure_changed = current.file.folder_id != target.file.folder_id
        || current.file.name != target.file.name
        || current.file.path != target.file.path;
    if structure_changed {
        history.push(FileHistoryDelta {
            entity: FileHistoryEntity::File,
            entity_id: file_id,
            operation: FileHistoryItemOperation::Move,
            before: Some(FileHistorySnapshot::File(current.file.history_snapshot())),
            after: Some(FileHistorySnapshot::File(target.file.history_snapshot())),
            ordinal: 0,
        });
        mutations.push(FileHistoryMutation::UpdateFileStructure {
            before: current.file.clone(),
            after: target.file.clone(),
        });
    }

    let mut stats_delta = MaterializedFileStats::default();
    for (entry_id, before) in current_by_id {
        let after = target_by_id.get(&entry_id).cloned().unwrap_or_else(|| {
            let mut tombstone = before.clone();
            tombstone.deleted = true;
            tombstone
        });
        if before == after {
            continue;
        }
        stats_delta += MaterializedFileStats::between_entries(&before, &after);
        let operation = match (before.deleted, after.deleted) {
            (true, false) => FileHistoryItemOperation::Restore,
            (false, true) => FileHistoryItemOperation::Tombstone,
            _ => FileHistoryItemOperation::Update,
        };
        let ordinal =
            i32::try_from(history.len()).map_err(|_| FileHistoryPlanError::InvalidTree)?;
        history.push(FileHistoryDelta {
            entity: FileHistoryEntity::Entry,
            entity_id: entry_id,
            operation,
            before: Some(FileHistorySnapshot::Entry(before.clone())),
            after: Some(FileHistorySnapshot::Entry(after.clone())),
            ordinal,
        });
        mutations.push(FileHistoryMutation::ReplaceEntry {
            entry_id,
            before,
            after,
        });
    }

    Ok(FileHistoryPlan {
        change_set_id,
        operation: FileHistoryOperation::Rollback,
        target: FileHistoryTarget::File(file_id),
        path_snapshot,
        source_change_set_id: Some(target_change_set_id),
        mutations,
        history,
        project_stats_delta: stats_delta,
        file_stats_delta: Some((file_id, stats_delta)),
        cp_delta_tenths: 0,
    })
}

/// 将当前 folder tree 回滚到目标版本的 parent/name，并为整棵当前树生成新路径 delta。
///
/// destination 使用目标 parent 的当前 active path，避免把历史中的陈旧 parent path
/// 重新写回；目标 parent id/name 仍来自服务端物化的历史版本。
#[allow(clippy::too_many_arguments)]
pub fn plan_folder_rollback(
    change_set_id: Uuid,
    target_change_set_id: Uuid,
    current_root: FolderNode,
    descendant_folders: Vec<FolderNode>,
    descendant_files: Vec<FileNode>,
    target_root: FolderNode,
    destination: Option<&FolderNode>,
    active_paths: &ActivePathIndex,
) -> Result<FileHistoryPlan, FileHistoryPlanError> {
    if current_root.id != target_root.id {
        return Err(FileHistoryPlanError::RollbackTargetMismatch);
    }
    if !current_root.is_active() || !target_root.is_active() {
        return Err(FileHistoryPlanError::RollbackTargetDeleted);
    }
    if target_root.parent_id != destination.map(|folder| folder.id) {
        return Err(FileHistoryPlanError::RollbackTargetMismatch);
    }
    let mut plan = plan_folder_move(
        change_set_id,
        current_root,
        descendant_folders,
        descendant_files,
        destination,
        &target_root.name,
        active_paths,
    )?;
    plan.operation = FileHistoryOperation::Rollback;
    plan.source_change_set_id = Some(target_change_set_id);
    plan.cp_delta_tenths = 0;
    Ok(plan)
}

enum StructureChange {
    Folder(FolderNode, FolderNode),
    File(FileNode, FileNode),
}

fn validate_name(name: &str) -> Result<(), FileHistoryPlanError> {
    if name.is_empty()
        || name == "."
        || name == ".."
        || name.contains('/')
        || name.contains('\\')
        || name.chars().any(char::is_control)
    {
        Err(FileHistoryPlanError::InvalidName)
    } else {
        Ok(())
    }
}

/// 校验一个文件/文件夹名称并生成规范化 child path。
///
/// API 与数据库层只传入 parent path；名称与路径段规则仍由 core 统一决定。
pub fn child_path(parent: Option<&str>, name: &str) -> Result<String, FileHistoryPlanError> {
    validate_name(name)?;
    Ok(join_path(parent, name))
}

fn validate_unique_ids(ids: impl IntoIterator<Item = i64>) -> Result<(), FileHistoryPlanError> {
    let mut seen = HashSet::new();
    for id in ids {
        if !seen.insert(id) {
            return Err(FileHistoryPlanError::DuplicateEntity { id });
        }
    }
    Ok(())
}

fn join_path(parent: Option<&str>, name: &str) -> String {
    parent.map_or_else(|| name.to_string(), |parent| format!("{parent}/{name}"))
}

fn path_is_descendant(candidate: &str, ancestor: &str) -> bool {
    candidate
        .strip_prefix(ancestor)
        .is_some_and(|suffix| suffix.starts_with('/'))
}

fn path_is_self_or_descendant(candidate: &str, ancestor: &str) -> bool {
    candidate == ancestor || path_is_descendant(candidate, ancestor)
}

fn replace_path_prefix(
    path: &str,
    old_root: &str,
    new_root: &str,
) -> Result<String, FileHistoryPlanError> {
    let suffix = path
        .strip_prefix(old_root)
        .filter(|suffix| suffix.starts_with('/'))
        .ok_or(FileHistoryPlanError::InvalidTree)?;
    Ok(format!("{new_root}{suffix}"))
}

fn require_operation_owner(
    actual: Option<Uuid>,
    expected: Uuid,
) -> Result<(), FileHistoryPlanError> {
    if actual == Some(expected) {
        Ok(())
    } else {
        Err(FileHistoryPlanError::OperationNotOwned { expected, actual })
    }
}

fn folder_map(folders: &[FolderNode]) -> Result<HashMap<i64, FolderNode>, FileHistoryPlanError> {
    validate_unique_ids(folders.iter().map(|folder| folder.id))?;
    Ok(folders
        .iter()
        .cloned()
        .map(|folder| (folder.id, folder))
        .collect())
}

fn folder_chain_active_after(
    start_id: i64,
    folders: &HashMap<i64, FolderNode>,
    restored_operation_id: Uuid,
) -> Result<bool, FileHistoryPlanError> {
    let mut current = Some(start_id);
    let mut seen = HashSet::new();
    while let Some(folder_id) = current {
        if !seen.insert(folder_id) {
            return Err(FileHistoryPlanError::InvalidTree);
        }
        let folder = folders
            .get(&folder_id)
            .ok_or(FileHistoryPlanError::InvalidTree)?;
        if folder
            .deletion_operation_id
            .is_some_and(|operation| operation != restored_operation_id)
        {
            return Ok(false);
        }
        current = folder.parent_id;
    }
    Ok(true)
}

fn folder_chain_is_active(
    start_id: i64,
    folders: &HashMap<i64, FolderNode>,
) -> Result<bool, FileHistoryPlanError> {
    let mut current = Some(start_id);
    let mut seen = HashSet::new();
    while let Some(folder_id) = current {
        if !seen.insert(folder_id) {
            return Err(FileHistoryPlanError::InvalidTree);
        }
        let folder = folders
            .get(&folder_id)
            .ok_or(FileHistoryPlanError::InvalidTree)?;
        if !folder.is_active() {
            return Ok(false);
        }
        current = folder.parent_id;
    }
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::upload_replacement::OriginalText;

    fn id(value: u128) -> Uuid {
        Uuid::from_u128(value)
    }

    fn stats(untranslated: i64, translated: i64) -> MaterializedFileStats {
        MaterializedFileStats {
            visible_total: untranslated + translated,
            untranslated,
            translated,
            ..MaterializedFileStats::default()
        }
    }

    fn folder(
        id: i64,
        parent_id: Option<i64>,
        path: &str,
        deletion_operation_id: Option<Uuid>,
    ) -> FolderNode {
        FolderNode {
            id,
            parent_id,
            name: path.rsplit('/').next().unwrap().to_string(),
            path: path.to_string(),
            deletion_operation_id,
        }
    }

    fn file(
        id: i64,
        folder_id: Option<i64>,
        path: &str,
        deletion_operation_id: Option<Uuid>,
        stats: MaterializedFileStats,
    ) -> FileNode {
        FileNode {
            id,
            folder_id,
            name: path.rsplit('/').next().unwrap().to_string(),
            path: path.to_string(),
            deletion_operation_id,
            stats,
        }
    }

    fn entry(id: i64, key: &str, state: EntryState, hidden: bool, deleted: bool) -> VersionedEntry {
        VersionedEntry {
            id,
            snapshot: EntryHistorySnapshot {
                key: key.to_string(),
                original: OriginalText::from([("en".to_string(), format!("source-{key}"))]),
                translation: format!("translation-{key}"),
                state,
                locked: false,
                hidden,
                questioned: false,
                deleted,
            },
        }
    }

    #[test]
    fn folder_move_rewrites_entire_tree_and_rejects_cycles_and_conflicts() {
        let root = folder(1, None, "chapter", None);
        let child = folder(2, Some(1), "chapter/one", None);
        let nested_file = file(10, Some(2), "chapter/one/a.json", None, stats(2, 1));
        let destination = folder(3, None, "archive", None);
        let active_paths = ActivePathIndex {
            folders: BTreeSet::from([
                "chapter".to_string(),
                "chapter/one".to_string(),
                "archive".to_string(),
            ]),
            files: BTreeSet::from(["chapter/one/a.json".to_string()]),
        };

        let plan = plan_folder_move(
            id(1),
            root.clone(),
            vec![child.clone()],
            vec![nested_file.clone()],
            Some(&destination),
            "renamed",
            &active_paths,
        )
        .unwrap();
        assert_eq!(plan.operation, FileHistoryOperation::Move);
        assert_eq!(plan.mutations.len(), 3);
        assert!(plan.mutations.iter().any(|mutation| matches!(
            mutation,
            FileHistoryMutation::UpdateFileStructure { after, .. }
                if after.path == "archive/renamed/one/a.json"
        )));
        assert_eq!(plan.project_stats_delta, MaterializedFileStats::default());

        assert_eq!(
            plan_folder_move(
                id(2),
                root.clone(),
                vec![child.clone()],
                vec![nested_file.clone()],
                Some(&child),
                "chapter",
                &active_paths,
            ),
            Err(FileHistoryPlanError::MoveIntoDescendant)
        );

        let mut conflicting = active_paths.clone();
        conflicting
            .files
            .insert("archive/renamed/one/a.json".to_string());
        assert_eq!(
            plan_folder_move(
                id(3),
                root,
                vec![child],
                vec![nested_file],
                Some(&destination),
                "renamed",
                &conflicting,
            ),
            Err(FileHistoryPlanError::PathConflict {
                path: "archive/renamed/one/a.json".to_string()
            })
        );
    }

    #[test]
    fn delete_marks_only_active_rows_and_subtracts_materialized_file_stats() {
        let delete_id = id(10);
        let prior_delete = id(9);
        let plan = plan_folder_delete(
            delete_id,
            1,
            vec![
                folder(1, None, "root", None),
                folder(2, Some(1), "root/active", None),
                folder(3, Some(1), "root/already-deleted", Some(prior_delete)),
            ],
            vec![
                file(10, Some(2), "root/active/a.json", None, stats(2, 3)),
                file(
                    11,
                    Some(3),
                    "root/already-deleted/b.json",
                    Some(prior_delete),
                    stats(7, 0),
                ),
            ],
        )
        .unwrap();

        assert_eq!(plan.operation, FileHistoryOperation::Delete);
        assert_eq!(plan.mutations.len(), 3);
        assert!(plan.mutations.iter().all(|mutation| match mutation {
            FileHistoryMutation::DeleteFolder { operation_id, .. }
            | FileHistoryMutation::DeleteFile { operation_id, .. } => {
                *operation_id == delete_id
            }
            _ => false,
        }));
        assert_eq!(plan.project_stats_delta, -stats(2, 3));
        assert!(!plan.history.iter().any(|delta| delta.entity_id == 3));
        assert!(!plan.history.iter().any(|delta| delta.entity_id == 11));
    }

    #[test]
    fn restore_is_owned_by_one_delete_operation_and_keeps_prior_deletions() {
        let restore_set = id(20);
        let deleted_tree = id(21);
        let prior_delete = id(22);
        let folders = vec![
            folder(1, None, "root", Some(deleted_tree)),
            folder(2, Some(1), "root/restored", Some(deleted_tree)),
            folder(3, Some(1), "root/prior", Some(prior_delete)),
        ];
        let plan = plan_folder_restore(
            restore_set,
            deleted_tree,
            1,
            folders,
            vec![
                file(
                    10,
                    Some(2),
                    "root/restored/a.json",
                    Some(deleted_tree),
                    stats(1, 2),
                ),
                file(
                    11,
                    Some(3),
                    "root/prior/b.json",
                    Some(prior_delete),
                    stats(8, 0),
                ),
            ],
            &ActivePathIndex::default(),
        )
        .unwrap();

        assert_eq!(plan.source_change_set_id, Some(deleted_tree));
        assert_eq!(plan.project_stats_delta, stats(1, 2));
        assert!(plan.history.iter().all(|delta| delta.entity_id != 3));
        assert!(plan.history.iter().all(|delta| delta.entity_id != 11));
        assert_eq!(plan.cp_delta_tenths, 0);

        let wrong_owner = plan_file_restore(
            id(23),
            deleted_tree,
            file(12, None, "root.json", Some(prior_delete), stats(1, 0)),
            &[],
            &ActivePathIndex::default(),
        );
        assert_eq!(
            wrong_owner,
            Err(FileHistoryPlanError::OperationNotOwned {
                expected: deleted_tree,
                actual: Some(prior_delete),
            })
        );
    }

    #[test]
    fn direct_file_restore_under_deleted_ancestor_is_rejected() {
        let inner_delete = id(30);
        let outer_delete = id(31);
        let plan = plan_file_restore(
            id(32),
            inner_delete,
            file(
                10,
                Some(2),
                "outer/inner/a.json",
                Some(inner_delete),
                stats(4, 1),
            ),
            &[
                folder(1, None, "outer", Some(outer_delete)),
                folder(2, Some(1), "outer/inner", Some(inner_delete)),
            ],
            &ActivePathIndex::default(),
        );
        assert_eq!(plan, Err(FileHistoryPlanError::AncestorDeleted));
    }

    #[test]
    fn rollback_materializes_current_to_target_as_new_zero_cp_delta() {
        let current_file = file(5, Some(2), "current/file.json", None, stats(0, 1));
        let target_file = file(5, Some(1), "target/file.json", None, stats(2, 0));
        let mut current_translation = entry(10, "kept", EntryState::Translated, false, false);
        current_translation.snapshot.translation = "new translation".to_string();
        let target_translation = entry(10, "kept", EntryState::Untranslated, false, false);
        let introduced_after_target = entry(11, "new", EntryState::Translated, false, false);
        let tombstone_to_restore = entry(12, "restore", EntryState::Translated, false, true);
        let target_restored = entry(12, "restore", EntryState::Translated, false, false);

        let plan = plan_file_rollback(
            id(40),
            id(39),
            MaterializedFileVersion {
                file: current_file,
                entries: vec![
                    current_translation,
                    introduced_after_target,
                    tombstone_to_restore,
                ],
            },
            MaterializedFileVersion {
                file: target_file,
                entries: vec![target_translation, target_restored],
            },
            &ActivePathIndex::default(),
        )
        .unwrap();

        assert_eq!(plan.operation, FileHistoryOperation::Rollback);
        assert_eq!(plan.source_change_set_id, Some(id(39)));
        assert_eq!(plan.cp_delta_tenths, 0);
        assert!(plan.history.iter().any(|delta| {
            delta.entity_id == 11 && delta.operation == FileHistoryItemOperation::Tombstone
        }));
        assert!(plan.history.iter().any(|delta| {
            delta.entity_id == 12 && delta.operation == FileHistoryItemOperation::Restore
        }));
        assert_eq!(
            plan.project_stats_delta,
            MaterializedFileStats {
                visible_total: 0,
                untranslated: 1,
                translated: -1,
                ..MaterializedFileStats::default()
            }
        );
    }

    #[test]
    fn rollback_tracks_hidden_state_changes_without_touching_visible_total() {
        let current_file = file(5, None, "file.json", None, MaterializedFileStats::default());
        let target_file = current_file.clone();
        let current = entry(10, "hidden", EntryState::Reviewed, true, false);
        let target = entry(10, "hidden", EntryState::Translated, true, false);

        let plan = plan_file_rollback(
            id(42),
            id(41),
            MaterializedFileVersion {
                file: current_file,
                entries: vec![current],
            },
            MaterializedFileVersion {
                file: target_file,
                entries: vec![target],
            },
            &ActivePathIndex::default(),
        )
        .unwrap();

        assert_eq!(
            plan.project_stats_delta,
            MaterializedFileStats {
                hidden_translated: 1,
                hidden_reviewed: -1,
                ..MaterializedFileStats::default()
            }
        );
        assert_eq!(plan.file_stats_delta, Some((5, plan.project_stats_delta)));
    }

    #[test]
    fn folder_rollback_rewrites_new_descendants_and_is_a_new_zero_cp_change_set() {
        let current = folder(1, Some(3), "archive/current", None);
        let child = folder(2, Some(1), "archive/current/new-child", None);
        let child_file = file(
            10,
            Some(2),
            "archive/current/new-child/file.json",
            None,
            stats(1, 0),
        );
        let target = FolderNode {
            id: 1,
            parent_id: None,
            name: "original".to_string(),
            path: "original".to_string(),
            deletion_operation_id: None,
        };
        let plan = plan_folder_rollback(
            id(50),
            id(49),
            current,
            vec![child],
            vec![child_file],
            target,
            None,
            &ActivePathIndex::default(),
        )
        .unwrap();
        assert_eq!(plan.operation, FileHistoryOperation::Rollback);
        assert_eq!(plan.source_change_set_id, Some(id(49)));
        assert_eq!(plan.cp_delta_tenths, 0);
        assert!(plan.mutations.iter().any(|mutation| matches!(
            mutation,
            FileHistoryMutation::UpdateFileStructure { after, .. }
                if after.path == "original/new-child/file.json"
        )));
    }

    #[test]
    fn before_after_payloads_have_explicit_allowlists() {
        assert_eq!(
            ENTRY_HISTORY_FIELDS,
            [
                "key",
                "original",
                "translation",
                "state",
                "locked",
                "hidden",
                "questioned",
                "deleted_at"
            ]
        );
        assert_eq!(
            FILE_HISTORY_FIELDS,
            ["folder_id", "name", "path", "deleted_at"]
        );
        assert_eq!(
            FOLDER_HISTORY_FIELDS,
            ["parent_id", "name", "path", "deleted_at"]
        );

        let serialized = serde_json::to_value(FolderHistorySnapshot {
            parent_id: None,
            name: "safe".to_string(),
            path: "safe".to_string(),
            deleted: true,
        })
        .unwrap();
        assert_eq!(
            serialized
                .as_object()
                .unwrap()
                .keys()
                .collect::<BTreeSet<_>>(),
            BTreeSet::from([
                &"name".to_string(),
                &"parent_id".to_string(),
                &"path".to_string()
            ])
        );
        assert!(!serialized.to_string().contains("context"));
        assert!(!serialized.to_string().contains("deletion_change_set_id"));
    }
}
