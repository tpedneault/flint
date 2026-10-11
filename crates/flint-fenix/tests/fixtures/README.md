# Fixtures

`fenix-home` is a Fenix folder as `FENIX_HOME` lays it out, with
`state/projects.json`, `state/recent.json` and `settings.toml` in the
shapes Fenix writes at the pinned revision (copied from a real Fenix
state folder, with the paths and values replaced). It keeps the awkward
cases real data has: recent files stored canonicalized (`\\?\C:\...`), a
UNC project (`\\?\UNC\...`), the same root under two spellings, a blank
group, the explorer's `dirs` key Flint ignores, and a Jira token Flint
must never read.
