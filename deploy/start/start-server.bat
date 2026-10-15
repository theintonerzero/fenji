@echo off
rem Starts the Fenji server in this folder, which holds its settings (fenji.toml) and its data
rem (data\): the first start writes both. On an error, the window stays open to show it.
cd /d "%~dp0"
fenji-server.exe %*
if errorlevel 1 pause
