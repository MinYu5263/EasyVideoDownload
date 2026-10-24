$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

if ($env:OS -ne 'Windows_NT')
{
    throw 'Run this script on Windows'
}

$projectRoot = (Resolve-Path -LiteralPath (Join-Path $PSScriptRoot '..')).Path
$package = Get-Content -LiteralPath (Join-Path $projectRoot 'package.json') -Raw | ConvertFrom-Json
$tauri = Get-Content -LiteralPath (Join-Path $projectRoot 'src-tauri/tauri.conf.json') -Raw | ConvertFrom-Json
$cargoVersionLine = Get-Content -LiteralPath (Join-Path $projectRoot 'src-tauri/Cargo.toml') |
        Where-Object { $_ -match '^version = "([^"]+)"$' } | Select-Object -First 1
$cargoVersion = [regex]::Match($cargoVersionLine, '^version = "([^"]+)"$').Groups[1].Value
$version = $package.version
if ($tauri.version -ne '../package.json' -or $version -ne $cargoVersion)
{
    throw 'The Tauri version source and Cargo version must match package.json'
}

$tauriCli = Join-Path $projectRoot 'node_modules/@tauri-apps/cli/tauri.js'
if (-not (Test-Path -LiteralPath $tauriCli -PathType Leaf))
{
    throw 'Install project dependencies before packaging'
}

# Clear the previous package output before building
$outputDirectory = [System.IO.Path]::GetFullPath((Join-Path $projectRoot 'dist'))
if (-not $outputDirectory.StartsWith($projectRoot + [System.IO.Path]::DirectorySeparatorChar, [System.StringComparison]::OrdinalIgnoreCase))
{
    throw 'The output directory must be inside the project'
}
if (Test-Path -LiteralPath $outputDirectory)
{
    $outputItem = Get-Item -LiteralPath $outputDirectory -Force
    if ($outputItem.Attributes -band [System.IO.FileAttributes]::ReparsePoint)
    {
        throw 'The output directory must not be a link'
    }
    Remove-Item -LiteralPath $outputDirectory -Recurse -Force
}

Push-Location -LiteralPath $projectRoot
try
{
    & node scripts/generate-icons.mjs
    if ($LASTEXITCODE -ne 0)
    {
        throw 'Icon generation failed'
    }

    & node $tauriCli build --bundles nsis
    if ($LASTEXITCODE -ne 0)
    {
        throw 'Tauri packaging failed'
    }

    $releaseRoot = Join-Path $projectRoot 'src-tauri/target/release'
    $executable = Join-Path $releaseRoot 'EasyVideoDownload.exe'
    $installerDirectory = Join-Path $releaseRoot 'bundle/nsis'
    $installers = @(Get-ChildItem -LiteralPath $installerDirectory -File -Filter "EasyVideoDownload_${version}_*-setup.exe")
    if ($installers.Count -ne 1 -or -not (Test-Path -LiteralPath $executable -PathType Leaf))
    {
        throw 'Expected one installer and the release executable'
    }

    $installerOutput = Join-Path $outputDirectory $installers[0].Name
    $portableOutput = Join-Path $outputDirectory ($installers[0].Name -replace '-setup\.exe$', '-portable.zip')
    Copy-Item -LiteralPath $installers[0].FullName -Destination $installerOutput -Force
    Compress-Archive -LiteralPath $executable -DestinationPath $portableOutput -CompressionLevel Optimal -Force

    foreach ($path in @($installerOutput, $portableOutput))
    {
        $artifact = Get-Item -LiteralPath $path
        if ($artifact.Length -eq 0)
        {
            throw "Empty package: $path"
        }
        $stream = [System.IO.File]::OpenRead($path)
        $sha256 = [System.Security.Cryptography.SHA256]::Create()
        try
        {
            $hash = [System.BitConverter]::ToString($sha256.ComputeHash($stream)).Replace('-', '')
        }
        finally
        {
            $sha256.Dispose()
            $stream.Dispose()
        }
        Write-Output "$path (SHA256: $hash)"
    }
}
finally
{
    Pop-Location
}
