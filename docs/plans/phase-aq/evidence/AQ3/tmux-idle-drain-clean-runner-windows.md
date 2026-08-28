# AQ3 tmux idle-transition-drain evidence

Host: `clean-runner-windows`
Commit: `35a28588ab4aa319eabbb972ca68931d9a61f05e`
Status: **SKIPPED_NO_TMUX**

This host has no `tmux` binary (for example a Windows runner). The live tmux idle-drain scenario needs a real tmux server and cannot execute here; this is a fail-closed skip, not a failure.
