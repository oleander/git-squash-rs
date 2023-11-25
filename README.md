# Git Soft Reset Tool

This tool helps you squash a specified number of commits into one with a more meaningful commit message. It provides a convenient way to squash commits without manually rebasing and interacting with the `git` command line.

## Features

- Retrieve and list the last `n` commits.
- Select a commit message from the past commits or input a new one.
- Squash the last `n` commits into a single commit with the selected message.

## Usage

```bash
$ cargo install --path .
$ git squash [number_of_commits]
```

Replace `[number_of_commits]` with the number of recent commits you want to squash.

## License

MIT License
