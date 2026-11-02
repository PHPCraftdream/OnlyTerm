## Installing on Windows

64-bit Windows 10.0.17763 or later is required to run OnlyTerm; running on
earlier versions of Windows is not possible, as OnlyTerm requires [Pseudo
Console support that was first released in Windows
10.0.17763](https://devblogs.microsoft.com/commandline/windows-command-line-introducing-the-windows-pseudo-console-conpty/).

Every release on the
[Releases page](https://github.com/PHPCraftdream/OnlyTerm/releases/latest)
has a setup.exe style installer (requires admin privileges) and a simple zip
file (no special privileges required).

[:fontawesome-brands-windows: Latest release :material-tray-arrow-down:](https://github.com/PHPCraftdream/OnlyTerm/releases/latest){ .md-button }

### Installer

`OnlyTerm-<version>-setup.exe` is produced with Inno Setup. It installs
OnlyTerm to your program files directory and registers that location in your
`PATH`. It also adds the Explorer entries *Open OnlyTerm here*, *OnlyTerm Run
As* and *OnlyTerm folder settings*, and associates `*.onlyterm-run` files with
OnlyTerm (see [Launching OnlyTerm](../launching.md)).

The installer can be run as a GUI to guide you through the install, but also
offers the [standard Inno Setup command line
options](https://jrsoftware.org/ishelp/index.php?topic=setupcmdline) to
configure/script the installation process.

### Zip

`OnlyTerm-windows-<version>.zip` can be extracted and run from anywhere,
including a flash drive for a portable/relocatable installation. It does not
register the Explorer entries or the file type.

1. Download the zip from the Releases page
2. Extract the zipfile and double-click `onlyterm-gui.exe` to run the UI
3. Configuration instructions can be [found here](../config/files.md)
