@echo off
setlocal
set "REPO=F:\github\atm-core-worktrees\evidence\readiness-1.5.0-windows"
set "USERPROFILE=C:\atm-bench\home-1.5.0"
set "HOME=C:\atm-bench\home-1.5.0"
set "ATM_HOME=C:\atm-bench\home-1.5.0\.atm"
set "ATM_CAPACITY_HOST_LABEL=windows-x64-01-isolated"
set "LOG=%REPO%\site\reports\issue-1171\lint-current-account-raw.log"
cd /d "%REPO%"
just lint > "%LOG%" 2>&1
set "CODE=%ERRORLEVEL%"
echo exit_code=%CODE%>> "%LOG%"
exit /b %CODE%
