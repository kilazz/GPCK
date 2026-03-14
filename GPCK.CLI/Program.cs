using GPCK.Core;
using Spectre.Console;
using Spectre.Console.Cli;
using System.ComponentModel;

namespace GPCK.CLI
{
    class Program
    {
        public static int Main(string[] args)
        {
            var app = new CommandApp();
            app.Configure(config =>
            {
                config.SetApplicationName("GPCK");
                config.AddCommand<CompressCommand>("compress")
                    .WithAlias("pack")
                    .WithDescription("Compress a folder into a .gtoc archive.");

                config.AddCommand<DecompressCommand>("decompress")
                    .WithAlias("unpack")
                    .WithDescription("Decompress a .gtoc archive.");

                config.AddCommand<VerifyCommand>("verify")
                    .WithDescription("Verify the integrity (CRC/Hash) of an archive.");

                config.AddCommand<InfoCommand>("info")
                    .WithDescription("Show technical details about an archive.");

                config.AddCommand<PatchCommand>("patch")
                    .WithDescription("Create a delta patch based on an existing archive.");
            });

            return app.Run(args);
        }
    }

    public class CompressCommand : AsyncCommand<CompressCommand.Settings>
    {
        public class Settings : CommandSettings
        {
            [CommandArgument(0, "<INPUT>")]
            [Description("Input folder or file.")]
            public string Input { get; set; } = "";

            [CommandArgument(1, "[OUTPUT]")]
            [Description("Output .gtoc file path.")]
            public string? Output { get; set; }

            [CommandOption("-l|--level")]
            [DefaultValue(9)]
            public int Level { get; set; }

            [CommandOption("-m|--method")]
            [DefaultValue(AssetPacker.CompressionMethod.Auto)]
            [Description("Compression method: Auto, Store, GDeflate, Zstd, LZ4")]
            public AssetPacker.CompressionMethod Method { get; set; }

            [CommandOption("--mip-split")]
            public bool MipSplit { get; set; }

            [CommandOption("--key")]
            public string? Key { get; set; }
        }

        public override async Task<int> ExecuteAsync(CommandContext context, Settings settings, CancellationToken cancellationToken)
        {
            string output = settings.Output ?? Path.ChangeExtension(settings.Input, ".gtoc");
            byte[]? keyBytes = !string.IsNullOrEmpty(settings.Key) ? System.Security.Cryptography.SHA256.HashData(System.Text.Encoding.UTF8.GetBytes(settings.Key)) : null;

            AnsiConsole.MarkupLine($"[bold green]Packing:[/] {settings.Input} -> {output} (Method: {settings.Method})");

            var packer = new AssetPacker();
            Dictionary<string, string> map;

            if (File.Exists(settings.Input))
            {
                // Single file mode
                map = new Dictionary<string, string>
                {
                    { settings.Input, Path.GetFileName(settings.Input) }
                };
            }
            else if (Directory.Exists(settings.Input))
            {
                // Directory mode
                map = AssetPacker.BuildFileMap(settings.Input);
            }
            else
            {
                AnsiConsole.MarkupLine($"[bold red]Error:[/] Input path '{settings.Input}' not found.");
                return 1;
            }

            await AnsiConsole.Progress()
                .StartAsync(async ctx =>
                {
                    var task = ctx.AddTask("[green]Compressing assets...[/]");
                    var progress = new Progress<int>(p => task.Value = p);

                    await packer.CompressFilesToArchiveAsync(
                        map, output, true, settings.Level, keyBytes, settings.MipSplit, progress, cancellationToken, settings.Method);
                });

            AnsiConsole.MarkupLine("[bold green]Done![/]");
            return 0;
        }
    }

    public class DecompressCommand : AsyncCommand<DecompressCommand.Settings>
    {
        public class Settings : CommandSettings
        {
            [CommandArgument(0, "<ARCHIVE>")]
            public string Archive { get; set; } = "";

            [CommandArgument(1, "[OUTPUT]")]
            public string? Output { get; set; }

            [CommandOption("--key")]
            public string? Key { get; set; }
        }

        public override async Task<int> ExecuteAsync(CommandContext context, Settings settings, CancellationToken cancellationToken)
        {
            string outDir = settings.Output ?? Path.GetFileNameWithoutExtension(settings.Archive);
            byte[]? keyBytes = !string.IsNullOrEmpty(settings.Key) ? System.Security.Cryptography.SHA256.HashData(System.Text.Encoding.UTF8.GetBytes(settings.Key)) : null;

            AnsiConsole.MarkupLine($"[bold blue]Unpacking:[/] {settings.Archive} -> {outDir}");

            var packer = new AssetPacker();

            await AnsiConsole.Progress()
                .StartAsync(async ctx =>
                {
                    var task = ctx.AddTask("[blue]Extracting...[/]");
                    var progress = new Progress<int>(p => task.Value = p);

                    await packer.DecompressArchiveAsync(settings.Archive, outDir, keyBytes, progress);
                });

            return 0;
        }
    }

    public class VerifyCommand : AsyncCommand<VerifyCommand.Settings>
    {
        public class Settings : CommandSettings
        {
            [CommandArgument(0, "<ARCHIVE>")]
            public string Archive { get; set; } = "";
            [CommandOption("--key")]
            public string? Key { get; set; }
        }

        public override async Task<int> ExecuteAsync(CommandContext context, Settings settings, CancellationToken cancellationToken)
        {
            if (Directory.Exists(settings.Archive))
            {
                AnsiConsole.MarkupLine("[bold red]ERROR:[/] Input is a directory. Please provide the path to a .gtoc file.");
                return 1;
            }

            if (!File.Exists(settings.Archive))
            {
                AnsiConsole.MarkupLine($"[bold red]ERROR:[/] File '{settings.Archive}' not found.");
                return 1;
            }

            byte[]? keyBytes = !string.IsNullOrEmpty(settings.Key) ? System.Security.Cryptography.SHA256.HashData(System.Text.Encoding.UTF8.GetBytes(settings.Key)) : null;

            return await AnsiConsole.Status()
                .StartAsync("Verifying integrity...", async ctx =>
                {
                    // Assuming VerifyArchive is blocking, we wrap it
                    bool result = await Task.Run(() => new AssetPacker().VerifyArchive(settings.Archive, keyBytes), cancellationToken);
                    if (result)
                    {
                        AnsiConsole.MarkupLine("[bold green]VERIFICATION PASSED[/]");
                        return 0;
                    }
                    else
                    {
                        AnsiConsole.MarkupLine("[bold red]VERIFICATION FAILED[/]");
                        return 1;
                    }
                });
        }
    }

    public class InfoCommand : AsyncCommand<InfoCommand.Settings>
    {
        public class Settings : CommandSettings { [CommandArgument(0, "<ARCHIVE>")] public string Archive { get; set; } = ""; }

        public override Task<int> ExecuteAsync(CommandContext context, Settings settings, CancellationToken cancellationToken)
        {
            var info = new AssetPacker().InspectPackage(settings.Archive);

            var grid = new Grid();
            grid.AddColumn();
            grid.AddColumn();
            grid.AddRow("Version", info.Version.ToString());
            grid.AddRow("Files", info.FileCount.ToString());
            grid.AddRow("Total Size", $"{info.TotalSize / 1024.0 / 1024.0:F2} MB");

            AnsiConsole.Write(new Panel(grid).Header("Archive Info"));

            var table = new Table();
            table.AddColumn("Path");
            table.AddColumn("Size");
            table.AddColumn("Comp %");
            table.AddColumn("Method");

            foreach (var e in info.Entries)
            {
                double ratio = e.OriginalSize > 0 ? (double)e.CompressedSize / e.OriginalSize * 100 : 0;
                table.AddRow(
                    e.Path.Length > 50 ? "..." + e.Path.Substring(e.Path.Length - 47) : e.Path,
                    $"{e.OriginalSize / 1024} KB",
                    $"{ratio:F0}%",
                    e.Method
                );
            }
            AnsiConsole.Write(table);
            return Task.FromResult(0);
        }
    }

    public class PatchCommand : AsyncCommand<PatchCommand.Settings>
    {
        public class Settings : CommandSettings
        {
            [CommandArgument(0, "<BASE>")] public string Base { get; set; } = "";
            [CommandArgument(1, "<CONTENT>")] public string Content { get; set; } = "";
            [CommandArgument(2, "<OUTPUT>")] public string Output { get; set; } = "";
        }

        public override async Task<int> ExecuteAsync(CommandContext context, Settings settings, CancellationToken cancellationToken)
        {
            AnsiConsole.MarkupLine($"Creating patch [bold]{settings.Output}[/] from [bold]{settings.Content}[/] against [bold]{settings.Base}[/]");

            var packer = new AssetPacker();
            var map = AssetPacker.BuildFileMap(settings.Content);

            await AnsiConsole.Progress().StartAsync(async ctx =>
            {
                var t = ctx.AddTask("Computing Deltas & Packing...");
                await packer.CompressFilesToArchiveAsync(map, settings.Output, true, 9, null, false, null, cancellationToken);
                t.Value = 100;
            });
            return 0;
        }
    }
}
