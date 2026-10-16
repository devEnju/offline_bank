# The package kinds, for the scripts: the program each patches, the crate
# and linker script of its payload, its cargo features, the builder's
# profile files its name is hashed from (Get-PackageId.ps1), its folder, and
# its verify script. The payload is linked into build/intermediate/<kind>/.
# The test build is the Bank crate with a feature, in folders of its own.
# What a package must contain is not here: the verify scripts keep their own
# lists, independent of the builder.
@{
    bank = @{
        Program  = 'bank'
        Crate    = 'bank-payload'
        Linker   = 'patches/bank/link/bank15.ld'
        Features = @()
        Profiles = @('bank15.rs', 'bank15/offline.rs')
        Folder   = 'build/binary/bank'
        Verify   = 'Verify-BankPatch.ps1'
    }
    transporter = @{
        Program  = 'transporter'
        Crate    = 'transporter-payload'
        Linker   = 'patches/transporter/link/transporter15.ld'
        Features = @()
        Profiles = @('transporter15.rs')
        Folder   = 'build/binary/transporter'
        Verify   = 'Verify-TransporterPatch.ps1'
    }
    test = @{
        Program  = 'bank'
        Crate    = 'bank-payload'
        Linker   = 'patches/bank/link/bank15.ld'
        Features = @('test-build')
        Profiles = @('bank15.rs', 'bank15/offline.rs', 'bank15/test.rs')
        Folder   = 'build/test'
        Verify   = 'Verify-BankPatch.ps1'
    }
    migration = @{
        Program  = 'bank'
        Crate    = 'bank-migrate-payload'
        Linker   = 'patches/bank-migrate/link/bank15-migrate.ld'
        Features = @()
        Profiles = @('bank15.rs', 'bank15/migrate.rs')
        Folder   = 'build/migration'
        Verify   = 'Verify-BankPatch.ps1'
    }
    download = @{
        Program  = 'bank'
        Crate    = 'bank-download-payload'
        Linker   = 'patches/bank-download/link/bank15-download.ld'
        Features = @()
        Profiles = @('bank15.rs', 'bank15/download.rs')
        Folder   = 'build/download'
        Verify   = 'Verify-BankPatch.ps1'
    }
}
