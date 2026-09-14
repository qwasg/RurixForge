# Microsoft Visual C++ runtime

The unmodified x64 release runtime DLLs in `../bin` come from the installed
Microsoft Visual Studio 2022 Build Tools redistribution directory:
`VC/Redist/MSVC/14.44.35112/x64/Microsoft.VC143.CRT`.

They supply the native engine and game modules' Microsoft C/C++ runtime dependencies.
No system-wide installer, registry change, debug runtime or development tools are included.
The files retain their Microsoft signatures and copyright metadata.

Microsoft's [Visual Studio 2022 redistribution list](https://learn.microsoft.com/en-us/visualstudio/releases/2022/redistribution#visual-c-runtime-files)
identifies the release runtime files under `VC/redist` and the applicable license terms.
The installed distribution notice is also preserved as `Microsoft-VC-Redist.txt`.

This package targets Windows x64 with a working Vulkan graphics driver. Windows
system components and the user's GPU driver are supplied by Windows and the device vendor.
