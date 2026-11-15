param(
    [string]$OutputDirectory = "./backups",
    [string]$EnvFile = "deployment/.env",
    [string]$ProjectName = "rustchat"
)
$ErrorActionPreference = "Stop"
$directory = (New-Item -ItemType Directory -Force -Path $OutputDirectory).FullName
$name = "rustchat-$(Get-Date -Format yyyyMMdd-HHmmss).dump"
$compose = @("compose", "--project-name", $ProjectName, "--env-file", $EnvFile, "-f", "deployment/compose.yml")
docker @compose exec -T db pg_dump -U rustchat -d rustchat -Fc -f "/tmp/$name"
if ($LASTEXITCODE -ne 0) { throw "pg_dump failed" }
$container = (docker @compose ps -q db).Trim()
docker cp "${container}:/tmp/$name" (Join-Path $directory $name)
if ($LASTEXITCODE -ne 0) { throw "Could not copy backup from the database container" }
docker @compose exec -T db rm "/tmp/$name"
if ($LASTEXITCODE -ne 0) { throw "Could not remove the temporary backup from the database container" }
Write-Output "Backup written to $(Join-Path $directory $name)"
