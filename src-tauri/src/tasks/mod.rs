//! 任务领域服务层（M3, design D1）：任务列表、任务与子任务、重复、归档、附件、标签。
//! 所有函数以 tasks.db `Connection` 为操作对象；附件文件目录由调用方传入（`tasks.files/`）。

pub mod attachments;
pub mod lists;
pub mod tags;
pub mod tasks;

pub(crate) fn new_id() -> String {
    uuid::Uuid::new_v4().to_string()
}
