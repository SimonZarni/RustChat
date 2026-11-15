param(
    [Parameter(Mandatory=$true)][string]$BackupFile,
    [string]$EnvFile = "deployment/.env",
    [string]$ProjectName = "rustchat"
)
$ErrorActionPreference = "Stop"
$file = (Resolve-Path -LiteralPath $BackupFile).Path
$directory = Split-Path -Parent $file
$name = Split-Path -Leaf $file
Write-Warning "This replaces the RustChat database contents. Verify that this is the intended backup."
$compose = @("compose", "--project-name", $ProjectName, "--env-file", $EnvFile, "-f", "deployment/compose.yml")
docker @compose stop server
if ($LASTEXITCODE -ne 0) { throw "Could not stop server" }
$container = (docker @compose ps -q db).Trim()
docker cp $file "${container}:/tmp/$name"
if ($LASTEXITCODE -ne 0) { throw "Could not copy backup into the database container" }
docker @compose exec -T db pg_restore -U rustchat -d rustchat --clean --if-exists --no-owner "/tmp/$name"
if ($LASTEXITCODE -ne 0) { throw "pg_restore failed" }
docker @compose exec -T db rm "/tmp/$name"
if ($LASTEXITCODE -ne 0) { throw "Could not remove the temporary backup from the database container" }
docker @compose start server
if ($LASTEXITCODE -ne 0) { throw "Could not restart the server" }
