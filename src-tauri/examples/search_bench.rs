//! Release-mode end-to-end aggregate search with 100,000 persisted pages.
use std::time::Instant;
use tenjee_vault_lib::{commands::{AppState,global_search::global_search},db::{layout,registry,startup},notes::hierarchy};
fn main() {
    let directory=tempfile::tempdir().unwrap();
    let root=layout::data_root(directory.path());
    let app=AppState::init(root.clone(),startup::startup(&root).unwrap()).unwrap();
    let space=app.inner.with_meta(registry::list_spaces).unwrap().remove(0);
    app.inner.with_space(&space.id,|conn| {
        let notebook=hierarchy::create_notebook(conn,"Benchmark",None)?;
        let section=hierarchy::create_section(conn,&notebook.id,None,"Pages",None)?;
        let tx=conn.transaction()?;
        { let mut insert=tx.prepare("INSERT INTO pages(id,section_id,title,content) VALUES (?1,?2,?3,?4)")?;
          for index in 0..100_000 { insert.execute(rusqlite::params![format!("page-{index}"),section.id,format!("Benchmark page {index}"),format!("searchneedle reference document {index}")])?; }
        }
        tx.commit()?; Ok(())
    }).unwrap();
    let mut samples=Vec::new();
    for _ in 0..5 {
        let started=Instant::now();
        let hits=global_search(&app.inner,"searchneedle",30).unwrap();
        assert!(!hits.is_empty());
        samples.push(started.elapsed().as_secs_f64()*1000.0);
    }
    let maximum=samples.iter().copied().fold(0.0,f64::max);
    println!("{}",serde_json::json!({"metric":"aggregate_search","pages":100000,"samples_ms":samples,"maximum_ms":maximum,"threshold_ms":300,"passed":maximum<300.0}));
    if maximum>=300.0 {std::process::exit(1);}
}
