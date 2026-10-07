# Publishing library artifacts

A library build stages its binary, header, source interface, and link arguments in
a private temporary directory beside the requested output. This puts all files
on the destination filesystem. The compiler checks every destination before
building and again before publication. Destinations must be ordinary files or
absent; symlinks, directories, and paths that would overwrite any loaded source
or imported interface are rejected.

Compilation, native tool failures, invalid arguments, and staging failures leave
the previous artifact set intact. Publication uses individual atomic renames,
keeping previous files as backups until the full set is installed. An ordinary
installation error rolls back completed replacements and removes newly added
outputs. If rollback itself fails, the diagnostic identifies the retained recovery
directory instead of deleting its backups.

This is not a crash-atomic multi-file transaction. Build systems must exclude
concurrent writers and coordinate readers while publishing a set. A process crash
or power loss can interrupt publication; compatibility fingerprints and explicit
verification help detect a mixed set afterward.

Interface extraction uses the same staging and replacement mechanism for its
single output. It rejects symlink destinations and cannot overwrite its input
binary. Native tools receive paths as separate arguments, including version-script
paths containing spaces or commas.
