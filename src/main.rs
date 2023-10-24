use dialoguer::{theme::ColorfulTheme, Input, Select};
use git2::{Commit, ObjectType, Repository, Sort};
use std::process::Command;

const MAX_COMMITS: usize = 20;

fn choice(commit: &Commit) -> String {
  let message = commit.summary().unwrap_or_default().to_string();
  let time_ago = hours_ago(commit.time().seconds());
  let hash = commit.id().to_string()[0..7].to_string();

  format!("★ {} {} {}", hash, time_ago, message)
}

fn hours_ago(time: i64) -> String {
  let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs() as i64;
  format!("{: <8}", format!("{} h", ((now - time) / 3600)))
}

fn walk(repo: &Repository, start_id: git2::Oid, count: usize) -> Vec<Commit> {
  let mut revwalk = repo.revwalk().unwrap();
  revwalk.set_sorting(Sort::TOPOLOGICAL).unwrap();
  revwalk.push(start_id).unwrap();

  revwalk.take(count).filter_map(Result::ok).filter_map(|oid| repo.find_commit(oid).ok()).collect()
}

fn main() {
  let repo = Repository::open_from_env().expect("Failed to find a repository");
  let head_id = repo.head().unwrap().target().unwrap();

  if std::env::args().len() == 1 {
    let commits = walk(&repo, head_id, MAX_COMMITS);

    let selection = Select::with_theme(&ColorfulTheme::default())
      .with_prompt("Choose a commit to squash")
      .items(&commits.iter().map(|c| choice(c)).collect::<Vec<_>>())
      .default(0)
      .interact()
      .unwrap();

    Command::new(std::env::current_exe().unwrap()).arg(selection.to_string()).spawn().unwrap();
    return;
  }

  let amount = std::env::args().nth(1).unwrap().parse::<usize>().expect("Failed to parse the amount");

  let commits = walk(&repo, head_id, amount);

  let commit_messages: Vec<String> = commits.iter().filter_map(|c| c.summary().map(|s| s.to_string())).collect();
  let mut items = vec!["➜ [Enter] Custom commit message".to_string()];
  items.extend_from_slice(&commit_messages);

  let selection = Select::with_theme(&ColorfulTheme::default())
    .with_prompt("Select a commit message")
    .items(&items)
    .default(0)
    .interact()
    .unwrap();

  let message = if selection == 0 {
    Input::<String>::with_theme(&ColorfulTheme::default())
      .with_prompt("Message")
      .validate_with(
        |input: &String| {
          if input.is_empty() {
            Err("Commit message is required".to_string())
          } else {
            Ok(())
          }
        },
      )
      .interact()
      .unwrap()
  } else {
    items[selection].clone()
  };

  Command::new("git").arg("reset").arg("--soft").arg(&format!("HEAD~{}", amount)).spawn().unwrap();
  Command::new("git").arg("commit").arg("--status").arg("-m").arg(&message).spawn().unwrap();
}
