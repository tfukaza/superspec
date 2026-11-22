param(
    [string]$OutputDirectory = 'generated/demos/critical-power-art'
)

$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.Drawing
$output = New-Item -ItemType Directory -Force -Path $OutputDirectory
$circuits = @(
    @{ Number = '01'; Color = '#FF5B59' },
    @{ Number = '02'; Color = '#FF9B4D' },
    @{ Number = '03'; Color = '#FFDB61' }
)
$titleFont = [Drawing.Font]::new('BIZ UDGothic', 100, [Drawing.FontStyle]::Bold, [Drawing.GraphicsUnit]::Pixel)
$warningFont = [Drawing.Font]::new('BIZ UDGothic', 36, [Drawing.FontStyle]::Regular, [Drawing.GraphicsUnit]::Pixel)
$numberFont = [Drawing.Font]::new('Consolas', 105, [Drawing.FontStyle]::Bold, [Drawing.GraphicsUnit]::Pixel)
$format = [Drawing.StringFormat]::GenericTypographic
$paper = [Drawing.SolidBrush]::new([Drawing.ColorTranslator]::FromHtml('#E5F0F1'))

try {
    foreach ($circuit in $circuits) {
        $bitmap = [Drawing.Bitmap]::new(1024, 256)
        $graphics = [Drawing.Graphics]::FromImage($bitmap)
        $accent = [Drawing.SolidBrush]::new([Drawing.ColorTranslator]::FromHtml($circuit.Color))
        $rule = [Drawing.Pen]::new($accent.Color, 3)
        try {
            $graphics.Clear([Drawing.ColorTranslator]::FromHtml('#142129'))
            $graphics.SmoothingMode = [Drawing.Drawing2D.SmoothingMode]::AntiAlias
            $graphics.TextRenderingHint = [Drawing.Text.TextRenderingHint]::AntiAliasGridFit
            $graphics.DrawRectangle($rule, 14, 14, 996, 228)
            $graphics.FillRectangle($accent, 30, 32, 9, 192)
            $graphics.DrawLine($rule, 83, 163, 705, 163)
            $graphics.DrawLine($rule, 739, 33, 739, 223)
            $graphics.DrawString('重要電源', $titleFont, $paper, 84, 30, $format)
            $graphics.DrawString('遮断禁止', $warningFont, $accent, 87, 178, $format)
            $graphics.DrawString($circuit.Number, $numberFont, $accent, 809, 62, $format)
            for ($i = 0; $i -lt 4; $i++) {
                $x = 795 + $i * 43
                $graphics.DrawLine($rule, $x, 207, ($x + 22), 187)
            }
            $bitmap.Save((Join-Path $output.FullName "critical-power-$($circuit.Number).png"), [Drawing.Imaging.ImageFormat]::Png)
        } finally {
            $rule.Dispose()
            $accent.Dispose()
            $graphics.Dispose()
            $bitmap.Dispose()
        }
    }
} finally {
    $paper.Dispose()
    $format.Dispose()
    $numberFont.Dispose()
    $warningFont.Dispose()
    $titleFont.Dispose()
}
