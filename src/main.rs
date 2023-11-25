#![allow(clippy::needless_borrow)]

use git2::{Commit, Repository, ResetType, Sort, Time};
use std::process::{ExitCode, Termination};
use anyhow::{bail, Context, Result};
use dialoguer::theme::ColorfulTheme;
use dialoguer::{Input, Select};
use std::path::Path;
use clap::Parser;

const MAX_MESSAGE_LENGTH: usize = 80;
const SECONDS_IN_HOUR: i64 = 3600;

struct Message(String);
impl Termination for Message {
  fn report(self) -> ExitCode {
    println!("{}", self.0);
    0.into()
  }
}

trait FormatCommit {
  fn format(&self) -> Result<String>;
}

impl<'a> FormatCommit for Commit<'a> {
  fn format(&self) -> Result<String> {
    let message = self.summary().unwrap_or_default().to_string();
    let hours = self.time().hours_ago();
    let mut formatted = format!("{} {}", hours, message);
    if formatted.len() > MAX_MESSAGE_LENGTH {
      formatted.truncate(MAX_MESSAGE_LENGTH);
      formatted.push_str("...");
    }
    Ok(formatted)
  }
}

trait Commitable {
  fn commit_with_msg(&self, message: &str) -> Result<git2::Oid>;
}

impl Commitable for Repository {
  fn commit_with_msg(&self, message: &str) -> Result<git2::Oid> {
    let mut index = self.index().context("Failed to get index")?;
    let oid = index.write_tree().context("Failed to write tree")?;
    let signature = self.signature().context("Failed to get signature")?;
    let tree = self.find_tree(oid).context("Failed to find tree")?;
    let parent = self.head().ok().and_then(|head| head.peel_to_commit().ok());
    let parents = parent.iter().collect::<Vec<&Commit>>();

    self
      .commit(Some("HEAD"), &signature, &signature, &message, &tree, parents.as_slice())
      .context("Could not commit")
  }
}

trait HoursAgo {
  fn hours_ago(&self) -> String;
}

impl HoursAgo for Time {
  fn hours_ago(&self) -> String {
    let now = std::time::SystemTime::now()
      .duration_since(std::time::UNIX_EPOCH)
      .unwrap()
      .as_secs() as i64;
    let hours = (now - self.seconds()) / SECONDS_IN_HOUR;
    format!("{: <8}", format!("{} h", hours))
  }
}

#[derive(Parser, Debug)]
#[clap(author, version, about)]
struct Cli {
  #[clap()]
  amount: usize,
}

fn iter_topological_commits(repo: &Repository, amount: usize) -> Result<impl Iterator<Item = Result<Commit, git2::Error>>> {
  let mut revwalk = repo.revwalk().context("Failed to get revwalk")?;
  revwalk.set_sorting(Sort::TOPOLOGICAL).context("Failed to set sorting")?;
  revwalk.push_head().context("Failed to push HEAD")?;

  Ok(
    revwalk
      .take(amount)
      .map(|oid_result| oid_result.and_then(|oid| repo.find_commit(oid).map_err(Into::into))),
  )
}

fn find_old_commit(repo: &Repository, amount: usize) -> Result<git2::Object> {
  iter_topological_commits(repo, amount + 1)?
    .last()
    .context("Failed to get last commit")
    .and_then(|commit| Ok(commit.map(|c| c.into_object())?))
}

fn git_soft_reset(repo: &Repository, amount: usize, message: &String) -> Result<git2::Oid> {
  let obj = find_old_commit(repo, amount).context("Failed to find old commit")?;
  repo.reset(&obj, ResetType::Soft, None).context("Failed to reset")?;
  repo.commit_with_msg(&message).context("Failed to commit")
}

fn commits(repo: &Repository, amount: usize) -> Result<Vec<Commit>> {
  Ok(
    iter_topological_commits(repo, amount)?
      .filter_map(Result::ok)
      .collect::<Vec<Commit>>(),
  )
}

fn validate_input(input: &String) -> Result<()> {
  if input.len() > MAX_MESSAGE_LENGTH {
    bail!("Message is too long, max is {}", MAX_MESSAGE_LENGTH);
  }

  Ok(())
}

fn prompt_for_commit_message() -> Result<String> {
  Input::<String>::with_theme(&ColorfulTheme::default())
    .with_prompt("Message")
    .validate_with(validate_input)
    .interact()
    .context("Failed to get commit message")
}

fn main() -> Result<Message> {
  let repo = Repository::open_ext(".", git2::RepositoryOpenFlags::empty(), Vec::<&Path>::new()).context("Failed to open repo")?;
  let mut items = vec!["➜ [Enter] Custom commit message".to_string()];
  let cli: Cli = Cli::parse();

  let messages: Vec<String> = commits(&repo, cli.amount)?
    .iter()
    .map(|c| c.format())
    .collect::<Result<Vec<String>>>()
    .context("Failed to format commits")?;

  items.extend_from_slice(&messages);

  let selection = Select::with_theme(&ColorfulTheme::default())
    .with_prompt("Select a commit message")
    .items(&items)
    .default(0)
    .interact()
    .context("Failed to et selection")?;

  let message = match selection {
    0 => prompt_for_commit_message(),
    n if n <= messages.len() => commits(&repo, cli.amount)?
      .get(n - 1)
      .context("Failed to get commit")?
      .message()
      .map(|s| s.to_string())
      .context("Failed to get commit message"),
    _ => bail!("Invalid selection"),
  }?;

  git_soft_reset(&repo, cli.amount, &message)?;

  Ok(Message(format!("Squashed {} commits", cli.amount)))
}

#[cfg(test)]
mod tests {
  use std::{fs::File, io::Write};

  use super::*;
  use git2::{Time, IndexAddOption};
  use log::{info, LevelFilter};
  use tempdir::TempDir;

  #[test]
  fn test_2_hours_ago() {
    let now = std::time::SystemTime::now()
      .duration_since(std::time::UNIX_EPOCH)
      .unwrap()
      .as_secs() as i64;
    let two_hours_ago = now - (SECONDS_IN_HOUR * 2);
    let hours = Time::new(two_hours_ago, 0).hours_ago();
    assert_eq!(hours.trim(), "2 h");
  }

  #[test]
  fn test_0_hours_ago() {
    let now = std::time::SystemTime::now()
      .duration_since(std::time::UNIX_EPOCH)
      .unwrap()
      .as_secs() as i64;
    let time = Time::new(now, 0);
    let hours = time.hours_ago();
    assert_eq!(hours.trim(), "0 h");
  }

  #[test]
  fn test_format_commit() {
    let repo = Repository::init("temp_test_repo").unwrap();
    let commit_id = repo.commit_with_msg("This is a test commit".as_ref()).unwrap();
    let commit = repo.find_commit(commit_id).unwrap();
    let formatted = commit.format().unwrap();
    assert!(formatted.contains("This is a test commit"));
    std::fs::remove_dir_all("temp_test_repo").unwrap();
  }

  #[test]
  fn test_get_commits() {
    let repo = Repository::init("temp_test_repo2").unwrap();
    let commit_id = repo.commit_with_msg("This is a test commit".as_ref()).unwrap();
    let commit = repo.find_commit(commit_id).unwrap();
    let commits = commits(&repo, 1).unwrap();
    assert_eq!(commits.len(), 1);
    assert_eq!(commits[0].id(), commit.id());

    // Cleanup
    std::fs::remove_dir_all("temp_test_repo2").unwrap();
  }

  #[test]
  fn test_commit_to_string() {
    let repo = Repository::init("temp_test_repo3").unwrap();
    let commit_id = repo.commit_with_msg("This is a test commit".as_ref()).unwrap();
    let commit = repo.find_commit(commit_id).unwrap();
    let commit_string = commit.summary().unwrap_or_default().to_string();
    assert_eq!(commit_string, "This is a test commit");

    // Cleanup
    std::fs::remove_dir_all("temp_test_repo3").unwrap();
  }

  // Create a test case using TempDir
  #[test]
  fn test_find_old_commit() -> Result<()> {
    let dir = TempDir::new("temp_test_repo4").unwrap();
    let repo = Repository::init(dir.path()).unwrap();

    for n in 0..10 {
      let name = format!("{}.txt", n);
      let file_path = dir.path().join(name.clone());
      let mut file = File::create(file_path).context("Failed to create file")?;
      let content = format!("{}", n);
      file.write_all(content.as_bytes()).context("Failed to write file")?;
      let message = format!("Commit {}", n);
      let mut index = repo.index().context("Failed to get index")?;
      index
        .add_all([name], IndexAddOption::DEFAULT, None)
        .context("Failed to add file")?;
      repo.commit_with_msg(message.as_ref()).context("Failed to commit")?;
    }

    let old_tree = repo.head().unwrap().peel_to_tree().unwrap();
    let new_commit = "New commit".to_string();
    git_soft_reset(&repo, 5, &new_commit).context("Failed to squash commits")?;
    let all_commits = commits(&repo, 10).unwrap();
    assert_eq!(all_commits.len(), 6);

    assert!(all_commits[0].message().unwrap().contains("New commit"));
    assert!(all_commits[1].message().unwrap().contains("4"));
    assert!(all_commits[2].message().unwrap().contains("3"));
    assert!(all_commits[3].message().unwrap().contains("2"));
    assert!(all_commits[4].message().unwrap().contains("1"));
    assert!(all_commits[5].message().unwrap().contains("0"));

    /* Check that all files exists */
    for n in 0..10 {
      let name = format!("{}.txt", n);
      let file_path = dir.path().join(name.clone());
      assert!(file_path.exists());
    }

    let new_tree = repo.head().unwrap().peel_to_tree().unwrap();
    let diff = repo.diff_tree_to_tree(Some(&old_tree), Some(&new_tree), None)?;

    env_logger::builder()
      .filter_level(LevelFilter::Debug)
      .format_target(false)
      .format_timestamp(None)
      .init();

    diff.print(git2::DiffFormat::Patch, |delta, _hunk, line| {
      let old = delta.old_file().path().unwrap();
      let new = delta.new_file().path().unwrap();
      info!("{} {}", old.display(), new.display());
      info!("{}", String::from_utf8_lossy(line.content()));
      true
    })?;

    Ok(())
  }

  #[test]
  fn test_commit_message_validation() {
    let long_message = "a".repeat(MAX_MESSAGE_LENGTH + 1);
    assert!(validate_input(&long_message).is_err());
  }

  #[test]
  fn test_commit_enumeration() -> Result<()> {
    let dir = TempDir::new("temp_test_repo_commit_enumeration").unwrap();
    let repo = Repository::init(dir.path()).unwrap();
    for n in 0..3 {
      let name = format!("{}.txt", n);
      let file_path = dir.path().join(name.clone());
      let mut file = File::create(file_path).context("Failed to create file")?;
      let content = format!("{}", n);
      file.write_all(content.as_bytes()).context("Failed to write file")?;
      let message = format!("Commit {}", n);
      let mut index = repo.index().context("Failed to get index")?;
      index
        .add_all([name], IndexAddOption::DEFAULT, None)
        .context("Failed to add file")?;
      repo.commit_with_msg(message.as_ref()).context("Failed to commit")?;
    }
    let commits_list = commits(&repo, 3)?;
    assert_eq!(commits_list.len(), 3);
    Ok(())
  }
}
