@echo off
chcp 65001 > nul
setlocal EnableExtensions

REM find_engine.bat (guid / version only) – safe debug logging (no redirection symbols)

set "DBG="
if /I "%FIND_ENGINE_DEBUG%"=="1" set "DBG=1"

set "TYPE=%~1"
set "VAL=%~2"

if not defined TYPE (1>&2 echo [find_engine] ERROR: missing TYPE & exit /b 1)
if not defined VAL  (1>&2 echo [find_engine] ERROR: missing VALUE & exit /b 1)

for /f "tokens=1,2 delims==" %%A in ("%TYPE%") do (
  if not "%%B"=="" (
    set "TYPE=%%~A"
    if not defined VAL set "VAL=%%~B"
  )
)

for %%K in (GUID Guid guid) do if /I "%%K"=="%TYPE%" set "TYPE=guid"
for %%K in (VERSION Version version) do if /I "%%K"=="%TYPE%" set "TYPE=version"

if /I "%TYPE%"=="path" (
  1>&2 echo [find_engine] ERROR: path mode not supported in this helper
  exit /b 1
)

if defined DBG 1>&2 echo [find_engine] TYPE=%TYPE% VAL=%VAL%

set "ENGINEPATH="

if /I "%TYPE%"=="guid" goto :RESOLVE_GUID
if /I "%TYPE%"=="version" goto :RESOLVE_VERSION

1>&2 echo [find_engine] ERROR: unknown TYPE=%TYPE%
exit /b 1

:RESOLVE_GUID
set "GUIDVAL=%VAL%"
if not "%GUIDVAL:~0,1%%GUIDVAL:~-1%"=="{}" set "GUIDVAL={%GUIDVAL%}"
if defined DBG 1>&2 echo [find_engine] GUID=%GUIDVAL%
for /f "tokens=1,2,*" %%A in ('reg query "HKCU\Software\Epic Games\Unreal Engine\Builds" /v "%GUIDVAL%" 2^>nul') do (
  if /I "%%A"=="%GUIDVAL%" set "ENGINEPATH=%%C"
)
goto :POST

:RESOLVE_VERSION
set "VER=%VAL%"
if not defined VER (
  1>&2 echo [find_engine] ERROR: VER empty
  exit /b 1
)
if defined DBG 1>&2 echo [find_engine] Version=%VER%

call :TRY_KEY "HKLM\SOFTWARE\EpicGames\Unreal Engine\%VER%" && goto :POST
call :TRY_KEY "HKLM\SOFTWARE\WOW6432Node\EpicGames\Unreal Engine\%VER%" && goto :POST

if not defined ENGINEPATH (
  for %%P in (
    "C:\Program Files\Epic Games\UE_%VER%"
    "D:\Program Files\Epic Games\UE_%VER%"
    "C:\Epic\UE_%VER%"
    "D:\Epic\UE_%VER%"
    "C:\UnrealEngine\UE_%VER%"
    "D:\UnrealEngine\UE_%VER%"
    "C:\Unreal\UE_%VER%"
    "D:\Unreal\UE_%VER%"
  ) do (
    if exist "%%~P\Engine\Binaries" (
      set "ENGINEPATH=%%~P"
      if defined DBG 1>&2 echo [find_engine] Fallback %%~P
      goto :POST
    )
  )
)

:POST
if defined DBG 1>&2 echo [find_engine] Raw="%ENGINEPATH%"
if not defined ENGINEPATH (
  1>&2 echo [find_engine] ERROR: engine path not found
  exit /b 2
)

set "ENGINEPATH=%ENGINEPATH:"=%"

:TRIM
if "%ENGINEPATH:~-1%"=="\" set "ENGINEPATH=%ENGINEPATH:~0,-1%" & goto TRIM
if "%ENGINEPATH:~-1%"=="/" set "ENGINEPATH=%ENGINEPATH:~0,-1%" & goto TRIM

if not exist "%ENGINEPATH%\Engine\Binaries" (
  1>&2 echo [find_engine] ERROR: invalid structure "%ENGINEPATH%"
  exit /b 3
)

echo %ENGINEPATH%
endlocal & exit /b 0

:TRY_KEY
set "ENGINEPATH="
for /f "tokens=1,2,*" %%A in ('reg query "%~1" /v InstalledDirectory 2^>nul') do (
  if /I "%%A"=="InstalledDirectory" set "ENGINEPATH=%%C"
)
if defined DBG (
  if defined ENGINEPATH (
    1>&2 echo [find_engine] Registry hit
    1>&2 echo [find_engine]   key = "%~1"
    1>&2 echo [find_engine]   path= "%ENGINEPATH%"
  ) else (
    1>&2 echo [find_engine] Registry miss "%~1"
  )
)
if defined ENGINEPATH exit /b 0
exit /b 1
