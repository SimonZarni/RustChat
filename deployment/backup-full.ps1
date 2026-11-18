param(
    [string]$OutputDirectory = "./backups",
    [string]$EnvFile = "deployment/.env",
    [string]$ProjectName = "rustchat"
)
$ErrorActionPreference = "Stop"

function Read-DotEnv([string]$Path) {
    $values = @{}
    foreach ($line in Get-Content -LiteralPath $Path) {
        if ($line -match '^\s*([^#=\s]+)=(.*)$') { $values[$matches[1]] = $matches[2] }
    }
    return $values
}

$settings = Read-DotEnv $EnvFile
foreach ($key in @('RUSTCHAT_S3_ENDPOINT', 'RUSTCHAT_S3_REGION', 'RUSTCHAT_S3_BUCKET', 'RUSTCHAT_S3_ACCESS_KEY_ID', 'RUSTCHAT_S3_SECRET_ACCESS_KEY')) {
    if (-not $settings[$key]) { throw "$key must be configured to make a full backup" }
}
if (-not (Get-Command aws -ErrorAction SilentlyContinue)) { throw "AWS CLI is required for object-store snapshots" }

$stamp = Get-Date -Format yyyyMMdd-HHmmss
$snapshot = [IO.Path]::GetFullPath((Join-Path $OutputDirectory "rustchat-snapshot-$stamp"))
$databaseDirectory = Join-Path $snapshot 'database'
$objectDirectory = Join-Path $snapshot 'objects'
New-Item -ItemType Directory -Force -Path $databaseDirectory, $objectDirectory | Out-Null
$compose = @('compose', '--project-name', $ProjectName, '--env-file', $EnvFile, '-f', 'deployment/compose.yml')
$serverStopped = $false
$envNames = @('AWS_ACCESS_KEY_ID', 'AWS_SECRET_ACCESS_KEY', 'AWS_DEFAULT_REGION', 'AWS_S3_ADDRESSING_STYLE')
$previous = @{}
foreach ($name in $envNames) { $previous[$name] = [Environment]::GetEnvironmentVariable($name, 'Process') }

try {
    docker @compose stop server
    if ($LASTEXITCODE -ne 0) { throw "Could not stop the server service" }
    $serverStopped = $true

    & "$PSScriptRoot\backup.ps1" -OutputDirectory $databaseDirectory -EnvFile $EnvFile -ProjectName $ProjectName
    if ($LASTEXITCODE -ne 0) { throw "PostgreSQL backup failed" }

    $env:AWS_ACCESS_KEY_ID = $settings.RUSTCHAT_S3_ACCESS_KEY_ID
    $env:AWS_SECRET_ACCESS_KEY = $settings.RUSTCHAT_S3_SECRET_ACCESS_KEY
    $env:AWS_DEFAULT_REGION = $settings.RUSTCHAT_S3_REGION
    $env:AWS_S3_ADDRESSING_STYLE = 'path'
    aws s3 sync "s3://$($settings.RUSTCHAT_S3_BUCKET)" $objectDirectory --endpoint-url $settings.RUSTCHAT_S3_ENDPOINT --region $settings.RUSTCHAT_S3_REGION --no-progress
    if ($LASTEXITCODE -ne 0) { throw "Object-store backup failed" }

    $configFiles = @()
    foreach ($config in @(
        @{ Operation = 'get-bucket-cors'; File = 'cors.json' },
        @{ Operation = 'get-bucket-lifecycle-configuration'; File = 'lifecycle.json' }
    )) {
        $result = $null
        $configError = ''
        try {
            $result = & aws s3api $config.Operation --bucket $settings.RUSTCHAT_S3_BUCKET --endpoint-url $settings.RUSTCHAT_S3_ENDPOINT --region $settings.RUSTCHAT_S3_REGION --output json 2>&1
            $configExitCode = $LASTEXITCODE
        } catch {
            $configExitCode = 1
            $configError = $_.Exception.Message
        }
        if ($configExitCode -eq 0) {
            [IO.File]::WriteAllText((Join-Path $snapshot $config.File), ($result -join "`n"), [System.Text.UTF8Encoding]::new($false))
            $configFiles += $config.File
        } else {
            if (-not $configError) { $configError = $result -join "`n" }
            if ($configError -notmatch 'NoSuchCORSConfiguration|NoSuchLifecycleConfiguration|NoSuchLifecycleConfigurationException') {
                throw "Could not read bucket configuration '$($config.Operation)'"
            }
        }
    }

    $objects = @(Get-ChildItem -LiteralPath $objectDirectory -File -Recurse)
    $manifest = [ordered]@{
        format_version = 1
        created_at_utc = [DateTime]::UtcNow.ToString('o')
        source_endpoint = $settings.RUSTCHAT_S3_ENDPOINT
        source_bucket = $settings.RUSTCHAT_S3_BUCKET
        object_count = $objects.Count
        object_bytes = [int64](($objects | Measure-Object -Property Length -Sum).Sum)
        bucket_configuration = $configFiles
    }
    [IO.File]::WriteAllText((Join-Path $snapshot 'snapshot.json'), ($manifest | ConvertTo-Json -Depth 5), [System.Text.UTF8Encoding]::new($false))
    Write-Output "Full snapshot created at $snapshot"
}
finally {
    foreach ($name in $envNames) { [Environment]::SetEnvironmentVariable($name, $previous[$name], 'Process') }
    if ($serverStopped) {
        docker @compose start server
        if ($LASTEXITCODE -ne 0) { Write-Error "Snapshot finished but the server did not restart" }
    }
}
