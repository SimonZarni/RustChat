param(
    [Parameter(Mandatory=$true)][string]$SnapshotDirectory,
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

$snapshot = (Resolve-Path -LiteralPath $SnapshotDirectory).Path
$manifestPath = Join-Path $snapshot 'snapshot.json'
$objectDirectory = Join-Path $snapshot 'objects'
$databaseDirectory = Join-Path $snapshot 'database'
if (-not (Test-Path -LiteralPath $manifestPath -PathType Leaf) -or -not (Test-Path -LiteralPath $objectDirectory -PathType Container)) {
    throw "Snapshot is incomplete: snapshot.json and objects/ are required"
}
$manifest = Get-Content -LiteralPath $manifestPath -Raw | ConvertFrom-Json
if ($manifest.format_version -ne 1) { throw "Unsupported snapshot format version" }
$dumps = @(Get-ChildItem -LiteralPath $databaseDirectory -Filter '*.dump' -File)
if ($dumps.Count -ne 1) { throw "Snapshot must contain exactly one PostgreSQL dump" }

$settings = Read-DotEnv $EnvFile
foreach ($key in @('RUSTCHAT_S3_ENDPOINT', 'RUSTCHAT_S3_REGION', 'RUSTCHAT_S3_BUCKET', 'RUSTCHAT_S3_ACCESS_KEY_ID', 'RUSTCHAT_S3_SECRET_ACCESS_KEY')) {
    if (-not $settings[$key]) { throw "$key must be configured to restore a full snapshot" }
}
if (-not (Get-Command aws -ErrorAction SilentlyContinue)) { throw "AWS CLI is required to restore object-store data" }

$envNames = @('AWS_ACCESS_KEY_ID', 'AWS_SECRET_ACCESS_KEY', 'AWS_DEFAULT_REGION', 'AWS_S3_ADDRESSING_STYLE')
$previous = @{}
foreach ($name in $envNames) { $previous[$name] = [Environment]::GetEnvironmentVariable($name, 'Process') }
$env:AWS_ACCESS_KEY_ID = $settings.RUSTCHAT_S3_ACCESS_KEY_ID
$env:AWS_SECRET_ACCESS_KEY = $settings.RUSTCHAT_S3_SECRET_ACCESS_KEY
$env:AWS_DEFAULT_REGION = $settings.RUSTCHAT_S3_REGION
$env:AWS_S3_ADDRESSING_STYLE = 'path'
$compose = @('compose', '--project-name', $ProjectName, '--env-file', $EnvFile, '-f', 'deployment/compose.yml')
$serverStopped = $false
$restoreSucceeded = $false

try {
    $target = & aws s3api list-objects-v2 --bucket $settings.RUSTCHAT_S3_BUCKET --endpoint-url $settings.RUSTCHAT_S3_ENDPOINT --region $settings.RUSTCHAT_S3_REGION --max-keys 1 --query 'length(Contents[])' --output text
    if ($LASTEXITCODE -ne 0) { throw "Could not inspect the target bucket" }
    if ($target -and $target.Trim() -ne 'None' -and $target.Trim() -ne '0') {
        throw "Restore requires an empty target bucket; $($settings.RUSTCHAT_S3_BUCKET) contains objects"
    }

    Write-Warning "This replaces PostgreSQL contents and restores object data into the configured empty bucket."
    docker @compose stop server
    if ($LASTEXITCODE -ne 0) { throw "Could not stop the server service" }
    $serverStopped = $true

    aws s3 sync $objectDirectory "s3://$($settings.RUSTCHAT_S3_BUCKET)" --endpoint-url $settings.RUSTCHAT_S3_ENDPOINT --region $settings.RUSTCHAT_S3_REGION --no-progress
    if ($LASTEXITCODE -ne 0) { throw "Object-store restore failed; the server remains stopped" }

    if ($manifest.bucket_configuration -contains 'cors.json') {
        aws s3api put-bucket-cors --bucket $settings.RUSTCHAT_S3_BUCKET --cors-configuration "file://$(Join-Path $snapshot 'cors.json')" --endpoint-url $settings.RUSTCHAT_S3_ENDPOINT --region $settings.RUSTCHAT_S3_REGION
        if ($LASTEXITCODE -ne 0) { throw "Could not restore bucket CORS configuration; the server remains stopped" }
    }
    if ($manifest.bucket_configuration -contains 'lifecycle.json') {
        aws s3api put-bucket-lifecycle-configuration --bucket $settings.RUSTCHAT_S3_BUCKET --lifecycle-configuration "file://$(Join-Path $snapshot 'lifecycle.json')" --endpoint-url $settings.RUSTCHAT_S3_ENDPOINT --region $settings.RUSTCHAT_S3_REGION
        if ($LASTEXITCODE -ne 0) { throw "Could not restore bucket lifecycle configuration; the server remains stopped" }
    }

    $container = (docker @compose ps -q db).Trim()
    if (-not $container) { throw "Database container is not running" }
    $name = $dumps[0].Name
    docker cp $dumps[0].FullName "${container}:/tmp/$name"
    if ($LASTEXITCODE -ne 0) { throw "Could not copy the snapshot dump to PostgreSQL" }
    docker @compose exec -T db pg_restore -U rustchat -d rustchat --clean --if-exists --no-owner "/tmp/$name"
    if ($LASTEXITCODE -ne 0) { throw "PostgreSQL restore failed; the server remains stopped" }
    docker @compose exec -T db rm "/tmp/$name"
    if ($LASTEXITCODE -ne 0) { Write-Warning "Could not remove the temporary dump inside the database container" }
    $restoreSucceeded = $true
}
finally {
    foreach ($name in $envNames) { [Environment]::SetEnvironmentVariable($name, $previous[$name], 'Process') }
    if ($serverStopped -and $restoreSucceeded) {
        docker @compose start server
        if ($LASTEXITCODE -ne 0) { throw "Restore completed but the server did not restart" }
    } elseif ($serverStopped) {
        Write-Warning "Restore did not complete; the server remains stopped to avoid serving mismatched database and object data."
    }
}

Write-Output "Snapshot restored from $snapshot"
