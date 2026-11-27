# Claude Code — repo rules

## Commits must be DCO signed off

Pull requests run a DCO check. **Every commit you push needs a
`Signed-off-by:` trailer** that matches the committer, so always commit with
`-s`:

```bash
nu -c 'git commit -s -m "Subject" -m "Body"'
```

That covers amends (`git commit --amend -s`) and fixup commits too. Before
pushing, confirm each new commit is signed off:

```bash
nu -c 'git log --format="%h %(trailers:key=Signed-off-by)" origin/main..HEAD'
```

If an earlier commit on the branch is missing the trailer, `git rebase
--signoff <base>` adds it. That rewrites pushed history, so ask before
force-pushing.
