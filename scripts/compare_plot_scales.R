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

observed <- data.frame(x = c(1, 2), y = c(2, 3), g = c("B", "A"))
summary <- data.frame(at = c(1, 3), mean = c(2.5, 4), series = c("A", "C"))
layers <- ggplot(observed, aes(x, y, colour = g)) +
  geom_point() +
  geom_point(data = summary, aes(at, mean, colour = series), inherit.aes = FALSE) +
  scale_colour_manual(values = c(A = "#F8766D", B = "#00BFC4", C = "#7CAE00"))
b <- ggplot_build(layers)
stopifnot(identical(b$data[[1]]$colour, c("#00BFC4", "#F8766D")))
stopifnot(identical(b$data[[2]]$colour, c("#F8766D", "#7CAE00")))
cat("Independent layers and shared colour training: PASS\n")

transformed <- ggplot(data.frame(x = c(1, 10, 100), y = c(1, 4, 9)), aes(x, y)) +
  geom_point() + scale_x_log10() + scale_y_sqrt()
b <- ggplot_build(transformed)$data[[1]]
stopifnot(isTRUE(all.equal(b$x, c(0, 1, 2))))
stopifnot(isTRUE(all.equal(b$y, c(1, 2, 3))))
cat("Log10 and square-root position transforms: PASS\n")

ordered <- ggplot(data.frame(x = c("a", "b", "c"), y = 1:3), aes(x, y)) +
  geom_point() + scale_x_discrete(limits = c("c", "a", "unused"))
b <- ggplot_build(ordered)$data[[1]]
# ggplot2's discrete positions are one-based; std.plot's are zero-based.
stopifnot(isTRUE(all.equal(as.numeric(b$x), c(2, NA, 1))))
cat("Discrete order and excluded category: PASS\n")

faceted <- ggplot(data.frame(x = "A", site = c("North", "North", "South")), aes(x)) +
  geom_bar() + facet_wrap(vars(site))
b <- ggplot_build(faceted)
stopifnot(identical(as.numeric(b$data[[1]]$count), c(2, 1)))
stopifnot(length(b$layout$panel_scales_y) == 1L)
free <- ggplot_build(faceted + facet_wrap(vars(site), scales = "free_y"))
stopifnot(length(free$layout$panel_scales_y) == 2L)
cat("Per-panel counts and fixed/free y scales: PASS\n")

observations <- data.frame(x = c(1, 2, 1, 2), y = c(1, 2, 10, 20),
                           site = c("North", "North", "South", "South"))
reference <- data.frame(x = c(1, 2), y = c(5, 5))
b <- ggplot_build(ggplot(observations, aes(x, y)) + geom_point() +
  geom_line(data = reference) + facet_wrap(vars(site)))
stopifnot(nrow(b$data[[2]]) == 4L, length(unique(b$data[[2]]$PANEL)) == 2L)
cat("Layers without facet variables repeat in every panel: PASS\n")

intervals <- data.frame(x = 1:3, mean = c(2, 4, 3),
                        low = c(1, 2, 1.5), high = c(3, 6, 4.5))
b <- ggplot_build(ggplot(intervals, aes(x, mean, ymin = low, ymax = high)) +
  geom_errorbar(width = 0.2) + geom_ribbon(stat = "identity"))
stopifnot(identical(b$data[[1]]$ymin, intervals$low),
          identical(b$data[[1]]$ymax, intervals$high))
stopifnot(isTRUE(all.equal(b$data[[1]]$xmin, intervals$x - 0.1)))
stopifnot(identical(b$data[[2]]$ymin, intervals$low),
          identical(b$data[[2]]$ymax, intervals$high))
cat("Supplied uncertainty bounds and error-bar cap widths: PASS\n")
