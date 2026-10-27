# Optional reference check for the manual-palettes/custom-gradient SVG fixtures.
# Run with Rscript; ggplot2 must be installed in an R library. No files are written.
library(ggplot2)
cat("ggplot2", as.character(packageVersion("ggplot2")), "\n")
d <- data.frame(x = c("B", "A"), y = c(2, 3))
p <- ggplot(d, aes(x, y, colour = x, fill = x)) +
  geom_col(position = "identity") +
  scale_colour_manual(values = c(B = "#112233", A = "#445566")) +
  scale_fill_manual(values = c(A = "#abcdef", B = "#fedcba"))
b <- ggplot_build(p)$data[[1]]
stopifnot(identical(b$colour, c("#112233", "#445566")))
stopifnot(identical(b$fill, c("#fedcba", "#abcdef")))
cat("Named manual colour/fill mapping: PASS\n")
base <- ggplot(data.frame(x = 1, y = 1, category = "A"), aes(x, y, colour = category)) +
  geom_point() + scale_colour_manual(values = c(A = "red"))
changed <- suppressMessages(base + scale_colour_manual(values = c(A = "blue")))
stopifnot(ggplot_build(base)$data[[1]]$colour == "red")
stopifnot(ggplot_build(changed)$data[[1]]$colour == "blue")
cat("Immutable scale replacement: PASS\n")
ramp <- grDevices::colorRamp(c("#000000", "#FE8040"), space = "rgb")
rgb <- ramp(c(0, 0.5, 1))
hex <- apply(rgb, 1, function(v) sprintf("#%02x%02x%02x", as.integer(v[1]), as.integer(v[2]), as.integer(v[3])))
stopifnot(identical(hex, c("#000000", "#7f4020", "#fe8040")))
cat("RGB reference:", paste(hex, collapse = " "), "\n")
gradient <- ggplot(data.frame(x = 0:2, y = 1), aes(x, y, colour = x)) +
  geom_point() + scale_colour_gradient(low = "#000000", high = "#FE8040")
cat("ggplot2 Lab reference:", paste(ggplot_build(gradient)$data[[1]]$colour, collapse = " "), "\n")
