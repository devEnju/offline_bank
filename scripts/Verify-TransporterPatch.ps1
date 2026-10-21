<#
Independent check of the Transporter patch package in build/binary/transporter
(Verify-Package.ps1, which shares no code with patch-builder). Writes
verification.json into the package folder.

-Kind is as for Verify-BankPatch.ps1; Transporter has one package. Its edits
are written out below with their original words, in the order of the
builder's profile.
#>
[CmdletBinding()]
param(
    [ValidateSet('transporter')][string]$Kind = 'transporter',
    # Another copy of the package or its payload (Test-Verify.ps1).
    [string]$PackagePath = '',
    [string]$ElfPath = '',
    # The extracted original (Extract-Cia.ps1); by default the folder under
    # build/extracted whose code.bin has the reviewed hash, whatever its name.
    [string]$InputPath = ''
)
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
. (Join-Path $PSScriptRoot 'Verify-Package.ps1')

$program = @{
    Name = 'Transporter'; CodeLength = 0x22a000; TitleId = '00040000000c9c00'; TmdVersion = 5200; Remaster = 5
    CodeHash = '001c20ada74016507c969bb44a0a50f8edf803ec06a3fc46834263ba8df0fd2f'
    ExheaderHash = '4b6081a5d0242d43713fd0be3cec21ae73f075441a759af3ed03c5270d1baceb'
    Bootstrap = 0x0028D1AC; BootstrapEnd = 0x0028E000; Payload = 0x00364000
    AppInit = 0x00104644; StartupCall = 0x00103D9C
}
$edits = @(
    @(0x00103D9C, 0xEB000228L, 'bl:patch_bootstrap_startup'),
    @(0x00242EF8, 0xEBFFFF28L, 'bl:transporter_next'),
    @(0x002445EC, 0xE5940038L, 'bl:transporter_list_ready_stub'),
    @(0x00244EAC, 0xEB000090L, 'bl:transporter_list_indicator_stub'),
    @(0x00244D40, 0xE595005CL, 'b:transporter_list_frame_stub'),
    @(0x0019B50C, 0xE92D41F0L, 'b:transporter_message_stub'),
    @(0x0019B578, 0xEB01024EL, 'bl:transporter_message_text'),
    @(0x00246F48, 0xE5900000L, 'b:transporter_check_stub'),
    @(0x00248CDC, 0xE59F0304L, 'b:00248E44'),
    @(0x00248EBC, 0xE3A01022L, 0xE3A0100BL),
    @(0x00248F88, 0x0AFFFFF4L, 0x0AFFFFBAL),
    @(0x002455D0, 0xE92D4FF0L, 'b:transporter_read_stub'),
    @(0x00247304, 0xE92D4FF8L, 'b:transporter_question_stub'),
    @(0x0024A0C4, 0xE92D4070L, 'b:transporter_transfer_stub'),
    @(0x00246208, 0xEBFD5552L, 0xE3A00001L),
    @(0x00246278, 0xEBFD5536L, 0xE3A00001L),
    @(0x002462E8, 0xEBFD551AL, 0xE3A00001L),
    @(0x00245728, 0xE59F0C74L, 'b:00245800'),
    @(0x002460B8, 0xE59F02E4L, 'b:002461D0'),
    @(0x002488A0, 0xEBFD4CA3L, 0xE3A00001L),
    @(0x002458DC, 0xE7910007L, 'bl:transporter_slot_stub'),
    @(0x00247478, 0xE5940008L, 'b:002474E0'),
    @(0x00243528, 0xEBFF5D37L, 'bl:transporter_cart_id_stub'),
    @(0x002439A8, 0xEBFF5B8CL, 'bl:transporter_cart_read'),
    @(0x002437F4, 0xEBFF5CD5L, 'bl:transporter_cart_write'),
    @(0x0024397C, 0xEBFF5C73L, 'bl:transporter_cart_write'),
    @(0x00244F10, 0xE5846010L, 'bl:transporter_list_next_stub'),
    @(0x00244908, 0xE3A00000L, 'bl:transporter_select_stub'),
    @(0x00244F00, 0xEBFE654DL, 'bl:transporter_card_removed'),
    @(0x00245658, 0xEBFE6377L, 'bl:transporter_card_removed'),
    @(0x00248E68, 0xEBFE5573L, 'bl:transporter_card_removed'),
    @(0x0024A298, 0xEBFE5067L, 'bl:transporter_card_removed'),
    @(0x0024A474, 0xEBFE4FF0L, 'bl:transporter_card_removed'),
    @(0x0025C5A4, 0xE5940008L, 0xE1A00004L),
    @(0x0025C5A8, 0xEBFE07A3L, 'bl:transporter_entry_card_removed'),
    @(0x0025C5FC, 0xEBFF3A49L, 'bl:transporter_language_chosen'),
    @(0x0025C618, 0xEBFF3E93L, 0xE320F000L),
    @(0x0022BEE0, 0xEBFFFF49L, 'bl:transporter_language_order_stub'),
    @(0x0022B5B0, 0xEBFDDA8AL, 'bl:transporter_language_buttons'),
    @(0x0022B6E0, 0xE3500005L, 0xE3500007L),
    @(0x0022B6E4, 0x03A01001L, 0x03A01003L),
    @(0x0022B3B8, 0xEA000054L, 'b:transporter_language_back_stub'),
    @(0x0024BA44, 0xE3A07003L, 'bl:transporter_title_begin_stub'),
    @(0x0024BA74, 0xE3A03027L, 'bl:transporter_title_end_stub'),
    @(0x0024BAE8, 0xEBFE1E37L, 'bl:transporter_version_shown'),
    @(0x0013AEE4, 0xE1A01000L, 'bl:transporter_layout_check_stub'),
    @(0x001E41C0, 0xEBFCA8B7L, 'bl:transporter_vc_mount_stub'),
    @(0x0010621C, 0xEB0377DEL, 0xE320F000L),
    @(0x002412D4, 0xEB003FF3L, 'bl:transporter_vc_scan_stub'),
    @(0x0024A150, 0xE59F04F4L, 'b:transporter_deliver_stub'),
    @(0x0024A3C0, 0xE3A0000AL, 0xE3A0000EL),
    @(0x0024A3D0, 0xE3A0000CL, 'b:0024A2A8'),
    @(0x0024A4E8, 0x0AFFFFA7L, 0x0AFFFF6EL),
    @(0x0024F6B0, 0xEBFD35A1L, 'bl:transporter_shiny_build_stub'),
    @(0x0024F8E0, 0xE5D4101BL, 'bl:transporter_shiny_gender_stub'))
# SDK CreateThread (001E3F58) adds 0x20 to the worker's SDK argument before SVC 08.
$relies = @(@(0x001E3FC8, 0xE3550020L), @(0x001E3FD0, 0x92850020L), @(0x001E3FD8, 0x9A000008L), @(0x001E3FE4, 0xE3500027L), @(0x001E3FE8, 0x92800018L), @(0x001E3FF8, 0xE3500040L), @(0x001E3FFC, 0x83E00000L), @(0x001E403C, 0xAEF62B00L), @(0x001E4040, 0x93725B00L))

# What must stay the original's: the router, which the router entry calls
# (00242BA0..00242E08), and the two tables of the Virtual Console titles
# whose languages the hooks copied (39 rows of three words at 002AFE0C, one
# word each at 002C2B1C).
$protected = @(@(0x00242BA0, 0x268), @(0x002AFE0C, 39 * 12), @(0x002C2B1C, 39 * 4))

$extra = {
    param([byte[]]$original, [byte[]]$patched, $elf)
    # The layout check, called from 0013AEE4 before the payload can run, lies
    # beside the start-up hook and begins with the instruction it stands in
    # for (mov r1, r0).
    $check = $elf.symbols['transporter_layout_check_stub'].value
    Assert-Check ($check -ge $elf.boot.address -and $check + 4 -le $elf.boot.address + $elf.boot.file_bytes -and (U32 $patched ($check - 0x00100000)) -eq 0xE1A01000L) 'Layout check lies beside the start-up hook and begins with mov r1, r0'
    $text = [Text.Encoding]::ASCII.GetString($patched, $elf.rx.address - 0x00100000, $elf.rx.file_bytes)
    Assert-Check ($text.Contains('/mover.bin') -and $text.Contains('/mover.alt.bin')) 'Payload names /mover.bin and /mover.alt.bin'
    Assert-Check ($text.Contains('/roms/nds/saves/POKEMON_')) 'Payload names the SD save folder'
}

Test-Package -Kind $Kind -Program $program -Edits $edits -Relies $relies -Protected $protected -Router 'transporter_next' -Entries 31 -Extra $extra `
    -PackagePath $PackagePath -ElfPath $ElfPath -InputPath $InputPath `
    -Checks @('the layout check lies beside the start-up hook and begins with the instruction it stands in for', 'the payload names the transport-box files and the SD save folder')
