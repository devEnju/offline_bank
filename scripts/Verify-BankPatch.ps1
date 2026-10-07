<#
Independent check of one built development package. This script shares no code
with patch-builder: it parses the ELF and IPS itself, rebuilds the expected image
from hardcoded reviewed addresses, and compares every byte. It reads only local
build inputs and writes verification.json into the package folder.
#>
[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)][ValidatePattern('^[0-9a-f]{16}$')][string]$Package,
    [string]$ElfPath = 'build/intermediate/bank/bank-payload.elf'
)
Set-Location -LiteralPath ([IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..')))
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
function Assert-Check([bool]$condition, [string]$message) {
    if (-not $condition) { throw $message }
}
function U16([byte[]]$bytes,[int]$offset) { [BitConverter]::ToUInt16($bytes,$offset) }
function U32([byte[]]$bytes,[int]$offset) { [BitConverter]::ToUInt32($bytes,$offset) }
function Hash([byte[]]$bytes,[int]$offset=0,[int]$length=-1) {
    if($length -lt 0){$length=$bytes.Length-$offset}
    $algorithm=[Security.Cryptography.SHA256]::Create()
    try { [Convert]::ToHexString($algorithm.ComputeHash($bytes,$offset,$length)).ToLowerInvariant() }
    finally {$algorithm.Dispose()}
}
function CString([byte[]]$bytes,[int]$offset) {
    $end=[Array]::IndexOf[byte]($bytes,0,$offset)
    Assert-Check ($end -ge $offset) 'Unterminated ELF string'
    [Text.Encoding]::ASCII.GetString($bytes,$offset,$end-$offset)
}
$packagePath=Join-Path 'build/bank' $Package
$reportPath=Join-Path $packagePath 'verification.json'
try {
    $manifest=Get-Content -LiteralPath (Join-Path $packagePath 'manifest.json') -Raw | ConvertFrom-Json
    [byte[]]$original=[IO.File]::ReadAllBytes((Join-Path (Get-Location) 'build/extracted/559b2b6a6fd3b001/exefs/code.bin'))
    [byte[]]$originalHeader=[IO.File]::ReadAllBytes((Join-Path (Get-Location) 'build/extracted/559b2b6a6fd3b001/exheader.bin'))
    [byte[]]$elf=[IO.File]::ReadAllBytes((Join-Path (Get-Location) $ElfPath))
    [byte[]]$ips=[IO.File]::ReadAllBytes((Join-Path (Get-Location) (Join-Path $packagePath 'code.ips')))
    [byte[]]$header=[IO.File]::ReadAllBytes((Join-Path (Get-Location) (Join-Path $packagePath 'exheader.bin')))
    $codeHash=Hash $original
    $headerHash=Hash $originalHeader
    $elfHash=Hash $elf
    Assert-Check ($codeHash -eq '2dce4796f54807cf8a67f1ce6297bf472d969b30ed7a7e8e25c2a6c2bdc40abf') 'Original code hash'
    Assert-Check ($headerHash -eq '39d93584b07901dfa7ea2fc8ce1236cdd92476584ee2dbb60c11a4ae9402abf7') 'Original exheader hash'
    Assert-Check ($original.Length -eq 0x2ac000 -and $originalHeader.Length -eq 0x800) 'Original input lengths'
    Assert-Check ($manifest.source_code_sha256 -eq $codeHash -and $manifest.source_exheader_sha256 -eq $headerHash) 'Manifest input hashes'
    Assert-Check ($manifest.source_elf_sha256 -eq $elfHash -and $elfHash.StartsWith($Package)) 'Manifest ELF hash/package'
    Assert-Check ($manifest.title_id -eq '00040000000c9b00' -and $manifest.tmd_version -eq 6272 -and $manifest.remaster_version -eq 6) 'Manifest version'
    foreach($name in @('code.ips','exheader.bin')) {
        $bytes=if($name -eq 'code.ips'){$ips}else{$header}
        $artifact=$manifest.artifacts.$name
        Assert-Check ($artifact.sha256 -eq (Hash $bytes) -and $artifact.bytes -eq $bytes.Length) ('Artifact '+$name)
    }
    Assert-Check ($manifest.paired_files_required -eq $true -and $manifest.hardware_tested_by_builder -eq $false) 'Manifest validation status'

    # Parse ELF headers and symbols independently of patch-builder.
    Assert-Check ([Text.Encoding]::ASCII.GetString($elf,1,3) -eq 'ELF' -and $elf[0] -eq 0x7f -and $elf[4] -eq 1 -and $elf[5] -eq 1) 'ELF32 little-endian header'
    Assert-Check ((U16 $elf 16) -eq 2 -and (U16 $elf 18) -eq 40) 'ARM executable ELF'
    $phoff=U32 $elf 28; $phsize=U16 $elf 42; $phcount=U16 $elf 44
    $shoff=U32 $elf 32; $shsize=U16 $elf 46; $shcount=U16 $elf 48
    Assert-Check ($phsize -eq 32 -and $shsize -eq 40) 'ELF table sizes'
    $loads=@()
    for($index=0;$index -lt $phcount;$index++) {
        $at=$phoff+$index*$phsize
        if((U32 $elf $at) -ne 1){continue}
        $load=[pscustomobject]@{offset=(U32 $elf ($at+4));address=(U32 $elf ($at+8));file_bytes=(U32 $elf ($at+16));memory_bytes=(U32 $elf ($at+20));flags=(U32 $elf ($at+24));alignment=(U32 $elf ($at+28))}
        Assert-Check ($load.address -eq (U32 $elf ($at+12)) -and $load.file_bytes -le $load.memory_bytes) 'ELF LOAD mapping'
        Assert-Check ($load.offset+$load.file_bytes -le $elf.Length -and $load.alignment -eq 0x1000) 'ELF LOAD file range'
        Assert-Check (($load.address%0x1000) -eq ($load.offset%0x1000)) 'ELF LOAD congruence'
        $loads+=$load
    }
    $loads=@($loads | Sort-Object address)
    Assert-Check ($loads.Count -eq 3) 'Expected bootstrap/RX/RW LOADs'
    $boot=$loads[0]; $rx=$loads[1]; $rw=$loads[2]
    Assert-Check ($boot.address -eq 0x313910 -and $boot.flags -eq 5 -and $boot.memory_bytes -eq $boot.file_bytes -and $boot.address+$boot.memory_bytes -le 0x314000) 'Bootstrap LOAD'
    Assert-Check ($rx.address -eq 0x3fb000 -and $rx.flags -eq 5 -and $rx.memory_bytes -eq $rx.file_bytes -and $rx.memory_bytes%0x1000 -eq 0) 'Payload RX LOAD'
    Assert-Check ($rw.address -eq $rx.address+$rx.memory_bytes -and $rw.flags -eq 6 -and $rw.memory_bytes%0x1000 -eq 0) 'Payload RW LOAD'
    $imageEnd=$rw.address+$rw.memory_bytes
    $expandedSize=$imageEnd-0x100000
    Assert-Check ($expandedSize -eq $manifest.expanded_code_bytes -and $imageEnd -lt 0x08000000) 'Expanded allocation'
    $symbols=@{}
    $symbolsFound=0
    for($index=0;$index -lt $shcount;$index++) {
        $at=$shoff+$index*$shsize
        if((U32 $elf ($at+4)) -ne 2){continue}
        $symbolsFound++
        $symbolOffset=U32 $elf ($at+16); $symbolBytes=U32 $elf ($at+20); $stringIndex=U32 $elf ($at+24)
        Assert-Check ((U32 $elf ($at+36)) -eq 16 -and $symbolBytes%16 -eq 0) 'ELF symbol entries'
        $stringAt=$shoff+$stringIndex*$shsize
        $stringOffset=U32 $elf ($stringAt+16)
        for($entry=$symbolOffset;$entry -lt $symbolOffset+$symbolBytes;$entry+=16) {
            $name=CString $elf ($stringOffset+(U32 $elf $entry))
            if($name){$symbols[$name]=[pscustomobject]@{value=(U32 $elf ($entry+4));size=(U32 $elf ($entry+8));info=$elf[$entry+12];section=(U16 $elf ($entry+14))}}
        }
    }
    Assert-Check ($symbolsFound -eq 1) 'One ELF symbol table'
    $exportNames=@('bank_offline_next','bank_offline_load','bank_offline_save','bank_offline_validate_game','bank_offline_rewards','bank_offline_timestamp','bank_offline_dex_save_request','bank_offline_dex_records_update','bank_offline_dex_records_finish')
    Assert-Check ($exportNames.Count -eq 9 -and $manifest.runtime_exports -eq 9) 'Nine runtime exports'
    Assert-Check (@($symbols.Keys | Where-Object { $_ -like 'bank_offline_menu*' }).Count -eq 0) 'Removed menu wrappers still linked'
    $wrapperNames=@('bank_svc_close_handle','bank_svc_wait_thread','bank_svc_get_resource_limit','bank_svc_get_resource_limit_values','bank_svc_get_resource_current_values')
    foreach($name in $wrapperNames) {
        Assert-Check ($symbols.ContainsKey($name)) ('Missing system-call wrapper '+$name)
        $symbol=$symbols[$name]
        Assert-Check ($symbol.value%4 -eq 0 -and $symbol.value -ge $rx.address -and $symbol.value+$symbol.size -le $rx.address+$rx.file_bytes -and $symbol.info -eq 0x12 -and $symbol.size -gt 0) ('System-call wrapper '+$name)
    }
    Assert-Check (@($manifest.exports.psobject.Properties).Count -eq $exportNames.Count) 'Manifest export count'
    foreach($name in $exportNames) {
        Assert-Check ($symbols.ContainsKey($name)) ('Missing ELF export '+$name)
        $symbol=$symbols[$name]
        Assert-Check ($symbol.value -eq $manifest.exports.$name -and $symbol.value%4 -eq 0 -and $symbol.value -ge $rx.address -and $symbol.value+$symbol.size -le $rx.address+$rx.file_bytes -and $symbol.info -eq 0x12 -and $symbol.size -gt 0) ('ELF export '+$name)
    }
    Assert-Check ($symbols.bank_bootstrap_startup.value -eq $boot.address -and (U32 $elf 24) -eq $symbols.bank_offline_next.value) 'ELF entry points'
    foreach($bound in @(@('__bank_payload_start',$rx.address),@('__bank_payload_rx_end',$rw.address),@('__bank_payload_rx_size',$rx.memory_bytes),@('__bank_payload_end',$imageEnd),@('__bank_original_app_init',0x10494c))) {
        Assert-Check ($symbols[$bound[0]].value -eq $bound[1]) ('ELF bound '+$bound[0])
    }

    # Hardcoded reviewed native edits, separate from the builder implementation.
    $branches=@(
        @(0x29f338,0xe594002cL,0x29f4fc,$false),
        @(0x1d3bf4,0xe92d4ff3L,'bank_offline_timestamp',$false),
        @(0x1040a4,0xeb000228L,'bank_bootstrap_startup',$true),
        @(0x2a5a2c,0xebfffed3L,'bank_offline_next',$true),
        @(0x2d1034,0xebff074aL,'bank_offline_validate_game',$true)
    )
    # Main-menu locations edited by earlier packages. They must stay original.
    $retired=@(
        @(0x1d6554,0xeb0000efL),
        @(0x2b33a4,0xebfc853eL),
        @(0x2b33d4,0xebfc8532L),
        @(0x3617bc,0x002a6750L),
        @(0x3617dc,0x002a6c84L)
    )
    $pointers=@(
        @(0x33d30c,0x2a7578,'bank_offline_dex_save_request'),
        @(0x33d34c,0x2a71a8,'bank_offline_dex_records_finish'),
        @(0x3600cc,0x26d8ec,'bank_offline_dex_records_update'),
        @(0x361a08,0x2a9750,'bank_offline_rewards'),
        @(0x361be4,0x2ad1bc,'bank_offline_rewards'),
        @(0x361cfc,0x2ae568,'bank_offline_load'),
        @(0x361d0c,0x2b4a20,'bank_offline_load'),
        @(0x361ed4,0x2af460,'bank_offline_load'),
        @(0x361ee4,0x2b4a20,'bank_offline_load'),
        @(0x362034,0x2b1cf8,'bank_offline_save'),
        @(0x362044,0x2b4a20,'bank_offline_save'),
        # Task 0xb update (server check) -> the native "finished" stub.
        @(0x361e84,0x2af034,0x2af124)
    )
    Assert-Check ((U32 $original 0x1af124) -eq 0xe3a00001L -and (U32 $original 0x1af128) -eq 0xe12fff1eL) 'Native finished stub 002af124'
    [byte[]]$expected=[byte[]]::new($expandedSize)
    [Array]::Copy($original,0,$expected,0,$original.Length)
    [byte[]]$allowed=[byte[]]::new($original.Length)
    foreach($edit in $branches) {
        $address=[int]$edit[0];$offset=$address-0x100000
        Assert-Check ((U32 $original $offset) -eq $edit[1]) ('Original branch/prologue '+$address.ToString('x8'))
        $target=if($edit[2] -is [string]){$symbols[$edit[2]].value}else{$edit[2]}
        $delta=[int64]$target-$address-8
        Assert-Check ($delta%4 -eq 0 -and $delta -ge -0x2000000 -and $delta -lt 0x2000000) 'ARM branch reach'
        $opcode=if($edit[3]){0xeb000000L}else{0xea000000L}
        $word=[uint32]($opcode -bor (($delta -shr 2) -band 0xffffff))
        [Array]::Copy([BitConverter]::GetBytes($word),0,$expected,$offset,4)
        [Array]::Fill[byte]($allowed,1,$offset,4)
    }
    foreach($edit in $pointers) {
        $offset=$edit[0]-0x100000
        Assert-Check ((U32 $original $offset) -eq $edit[1]) ('Original pointer '+$edit[0].ToString('x8'))
        $target=if($edit[2] -is [string]){$symbols[$edit[2]].value}else{$edit[2]}
        [Array]::Copy([BitConverter]::GetBytes([uint32]$target),0,$expected,$offset,4)
        [Array]::Fill[byte]($allowed,1,$offset,4)
    }
    $bootOffset=$boot.address-0x100000
    Assert-Check ((Hash $original $bootOffset $boot.file_bytes) -eq (Hash ([byte[]]::new($boot.file_bytes)))) 'Original bootstrap padding'
    [Array]::Fill[byte]($allowed,1,$bootOffset,$boot.file_bytes)
    foreach($load in $loads) {
        [Array]::Copy($elf,$load.offset,$expected,$load.address-0x100000,$load.file_bytes)
    }

    # Apply IPS to dirty extension memory, never assuming allocation is zero.
    [byte[]]$patched=[byte[]]::new($expandedSize)
    [Array]::Fill[byte]($patched,0xa5)
    [Array]::Copy($original,0,$patched,0,$original.Length)
    [byte[]]$covered=[byte[]]::new($expandedSize)
    Assert-Check ([Text.Encoding]::ASCII.GetString($ips,0,5) -eq 'PATCH') 'IPS magic'
    $at=5;$records=0
    while($true) {
        Assert-Check ($at+3 -le $ips.Length) 'IPS truncated offset'
        if([Text.Encoding]::ASCII.GetString($ips,$at,3) -eq 'EOF'){$at+=3;break}
        $offset=([int]$ips[$at] -shl 16)-bor([int]$ips[$at+1] -shl 8)-bor[int]$ips[$at+2];$at+=3
        Assert-Check ($at+2 -le $ips.Length) 'IPS truncated length'
        $length=([int]$ips[$at] -shl 8)-bor[int]$ips[$at+1];$at+=2
        $rle=$length -eq 0
        if($rle) {
            Assert-Check ($at+3 -le $ips.Length) 'IPS truncated RLE'
            $length=([int]$ips[$at] -shl 8)-bor[int]$ips[$at+1];$value=$ips[$at+2];$at+=3
        }
        Assert-Check ($length -gt 0 -and $offset+$length -le $expandedSize) 'IPS bounds'
        Assert-Check ([Array]::IndexOf[byte]($covered,1,$offset,$length) -eq -1) 'Overlapping IPS records'
        if($offset -lt $original.Length) {
            Assert-Check ($offset+$length -le $original.Length) 'IPS native/extension boundary'
            Assert-Check ([Array]::IndexOf[byte]($allowed,0,$offset,$length) -eq -1) 'IPS writes an unapproved native region'
        }
        [Array]::Fill[byte]($covered,1,$offset,$length)
        if($rle){[Array]::Fill[byte]($patched,$value,$offset,$length)}
        else {
            Assert-Check ($at+$length -le $ips.Length) 'IPS truncated data'
            [Array]::Copy($ips,$at,$patched,$offset,$length);$at+=$length
        }
        $records++
    }
    Assert-Check ($at -eq $ips.Length) 'Unexpected IPS EOF trailer'
    Assert-Check ([Array]::IndexOf[byte]($covered,0,$original.Length,$expandedSize-$original.Length) -eq -1) 'Uninitialized extension byte'
    $patchedHash=Hash $patched
    Assert-Check ($patchedHash -eq (Hash $expected) -and $patchedHash -eq $manifest.expanded_code_sha256) 'Expanded image differs from independent expected image'
    foreach($load in $loads) {
        Assert-Check ((Hash $patched ($load.address-0x100000) $load.file_bytes) -eq (Hash $elf $load.offset $load.file_bytes)) 'ELF LOAD file bytes'
        $bss=$load.memory_bytes-$load.file_bytes
        if($bss -gt 0) {
            Assert-Check ((Hash $patched ($load.address-0x100000+$load.file_bytes) $bss) -eq (Hash ([byte[]]::new($bss)))) 'ELF BSS bytes'
        }
    }
    foreach($location in $retired) {
        $offset=$location[0]-0x100000
        Assert-Check ((U32 $original $offset) -eq $location[1]) ('Original menu word '+$location[0].ToString('x8'))
        Assert-Check ((U32 $patched $offset) -eq $location[1]) ('Menu location is not original '+$location[0].ToString('x8'))
        Assert-Check ([Array]::IndexOf[byte]($allowed,1,$offset,4) -eq -1 -and [Array]::IndexOf[byte]($covered,1,$offset,4) -eq -1) ('Menu location still patched '+$location[0].ToString('x8'))
    }
    $payloadFileOffset=$rx.address-0x100000
    $payloadMemory=$imageEnd-$rx.address
    $payloadImageLength=$rw.address+$rw.file_bytes-$rx.address
    Assert-Check ((Hash $patched $payloadFileOffset $payloadImageLength) -eq $manifest.payload_image_sha256) 'Payload image hash'
    $placement=$manifest.placement
    Assert-Check ($placement.payload_address -eq $rx.address -and $placement.payload_file_offset -eq $payloadFileOffset -and $placement.payload_memory_bytes -eq $payloadMemory -and $placement.payload_rx_bytes -eq $rx.memory_bytes -and $placement.payload_rw_address -eq $rw.address -and $placement.payload_rw_bytes -eq $rw.memory_bytes -and $placement.bootstrap_address -eq $boot.address -and $placement.bootstrap_bytes -eq $boot.file_bytes) 'Manifest placement'

    # Only the data page count, initialized byte count, and BSS size may change.
    [byte[]]$expectedHeader=$originalHeader.Clone()
    $dataAddress=U32 $originalHeader 0x30
    $dataBytes=$imageEnd-$dataAddress
    [Array]::Copy([BitConverter]::GetBytes([uint32]($dataBytes/0x1000)),0,$expectedHeader,0x34,4)
    [Array]::Copy([BitConverter]::GetBytes([uint32]$dataBytes),0,$expectedHeader,0x38,4)
    [Array]::Clear($expectedHeader,0x3c,4)
    Assert-Check ((Hash $header) -eq (Hash $expectedHeader)) 'Unexpected paired-exheader edit'
    $textPages=U32 $header 0x14;$roPages=U32 $header 0x24;$dataPages=U32 $header 0x34
    Assert-Check (($textPages+$roPages+$dataPages)*0x1000 -eq $expandedSize -and (U32 $header 0x3c) -eq 0) 'Paired allocation length/BSS'
    $originalBssStart=$dataAddress+(U32 $originalHeader 0x38)
    $originalBssEnd=$originalBssStart+(U32 $originalHeader 0x3c)
    Assert-Check ((($originalBssEnd+0xfff)-band -4096) -eq $rx.address) 'Payload follows original BSS'
    Assert-Check ((Hash $patched ($originalBssStart-0x100000) ($rx.address-$originalBssStart)) -eq (Hash ([byte[]]::new($rx.address-$originalBssStart)))) 'Former native BSS and alignment gap initialized'

    Assert-Check ($branches.Count+$pointers.Count+1 -eq 18 -and $manifest.native_edits -eq 18 -and $manifest.main_menu_edits -eq 0) 'Native region count'
    $report=[ordered]@{
        status='passed'; package=$Package; source_elf_sha256=$elfHash
        ips_sha256=(Hash $ips); paired_exheader_sha256=(Hash $header)
        ips_records=$records; native_regions=18; runtime_exports=$exportNames.Count
        system_call_wrappers=$wrapperNames.Count
        original_main_menu_locations=@($retired | ForEach-Object { $_[0].ToString('x8') })
        expanded_code_bytes=$expandedSize; expanded_code_sha256=$patchedHash
        initial_extension_fill='a5'; elf_loads=$loads
        checks=@('original inputs unchanged','all IPS records bounded and disjoint','extension fully initialized from nonzero memory','native bytes preserved outside 18 allowed regions','five earlier main-menu locations equal the original executable and receive no IPS record','no menu wrapper symbol remains; five system-call wrappers are in RX memory','all branch and pointer targets match independently parsed ELF symbols','all ELF LOAD file and BSS bytes match','former native BSS and alignment gap zeroed','only paired exheader fields 0x34/0x38/0x3c changed','paired allocation exactly covers expanded image','manifest hashes, exports, placement, and status match')
        limitations=@('static artifact verification; hardware behavior not established')
    }
    $report | ConvertTo-Json -Depth 8 | Set-Content -LiteralPath $reportPath -Encoding utf8
    $report | ConvertTo-Json -Depth 8
} catch {
    [ordered]@{status='failed';package=$Package;reason=$_.Exception.Message;line=$_.InvocationInfo.ScriptLineNumber} |
        ConvertTo-Json | Set-Content -LiteralPath $reportPath -Encoding utf8
    throw
}

