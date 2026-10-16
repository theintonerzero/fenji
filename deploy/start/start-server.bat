@echo off
rem Starts the Fenji server in this folder, which holds its settings (fenji.toml) and its data
rem (data\): the first start writes both. pushd, unlike cd, also works from a network share. On
rem any failure, a crash's negative exit code too, the window stays open to show it.
pushd "%~dp0"
fenji-server.exe %*
if %errorlevel% neq 0 pause
