# Packs mixins/ayynoxs_map_pack into dist/ayynoxs_map_pack-<version>.hnmod,
# the file the Horizon launcher installs from its Mods page.
$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.IO.Compression
Add-Type -AssemblyName System.IO.Compression.FileSystem

$id = 'ayynoxs_map_pack'
$version = '0.2.0'
$root = $PSScriptRoot
$bundle = Join-Path $root "mixins\$id"
$dist = Join-Path $root 'dist'
New-Item -ItemType Directory -Force $dist | Out-Null
$out = Join-Path $dist "$id-$version.hnmod"
if (Test-Path $out) { Remove-Item $out -Force }

$manifest = [ordered]@{
    game    = '26.3'
    horizon = 'd009ebd140476b5bdc958c00f56a75e82abec691'
    id      = $id
    mixins  = '0d6b103c70633d8fd9443d8d0acd3aadaacedb06'
    schema  = 1
    version = $version
} | ConvertTo-Json

$zip = [System.IO.Compression.ZipFile]::Open($out, 'Create')
try {
    $entry = $zip.CreateEntry('horizon-native.json')
    $w = New-Object System.IO.StreamWriter($entry.Open(), (New-Object System.Text.UTF8Encoding($false)))
    $w.Write($manifest); $w.Dispose()
    Get-ChildItem $bundle -File | Where-Object { $_.Extension -in '.rs', '.toml' } | ForEach-Object {
        [System.IO.Compression.ZipFileExtensions]::CreateEntryFromFile($zip, $_.FullName, "mixins/$id/$($_.Name)") | Out-Null
    }
} finally {
    $zip.Dispose()
}
Write-Host "Gepackt: $out"
