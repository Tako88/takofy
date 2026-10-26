# The repository's own settings

Not packaging, but the same phase: these are the settings the repository needs
once it is public, kept here so applying them is one command rather than a
memory of a web page.

## `main-ruleset.json`

Blocks force pushes to and deletion of `main`, and requires the two CI jobs
before anything lands on it.

```
gh api --method POST repos/Tako88/takofy/rulesets --input packaging/github/main-ruleset.json
```

**It cannot be applied yet.** On GitHub's free plan a private repository has no
branch protection and no rulesets — the API answers `403: Upgrade to GitHub Pro
or make this repository public` — so this waits on the repository being public.

The two contexts are the job names in `.github/workflows/ci.yml`, shown on the
commit as `Linux gates` and `Windows build`. Renaming a job renames the check,
and a rule naming a check that no longer exists blocks every merge.

`bypass_actors` lets a repository admin push anyway: a locally made `--no-ff`
merge commit has no CI run of its own, and without the bypass every phase close
would have to be merged on GitHub. Remove the `bypass_actors` entry to make the
checks binding for everyone.

Verify what is live with `gh api repos/Tako88/takofy/rulesets`, which lists the
ruleset and its id, and `gh api repos/Tako88/takofy/rulesets/<id>` for the rules
the API actually stored.
