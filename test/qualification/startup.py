"""Fresh-process import measurement used by the opt-in qualification suite."""

import json
import sys
from time import perf_counter_ns, process_time_ns


def main() -> None:
    start = perf_counter_ns()
    cpu = process_time_ns()
    from meedee import archive, ci, codecs, messages, midi, tuning, ump

    elapsed = perf_counter_ns() - start
    cpu_elapsed = process_time_ns() - cpu
    # Keep imports observable without opening a transport client.
    assert all((archive, ci, codecs, messages, midi, tuning, ump))
    peak_rss = None
    if sys.platform != "win32":
        import resource

        peak_rss = resource.getrusage(resource.RUSAGE_SELF).ru_maxrss
        if sys.platform != "darwin":
            peak_rss *= 1024
    print(
        json.dumps(
            {
                "module_import_wall_ns": elapsed,
                "module_import_cpu_ns": cpu_elapsed,
                "process_peak_rss_bytes": peak_rss,
                "rss_includes_native_allocations": True,
                "rss_includes_children": False,
            }
        )
    )


if __name__ == "__main__":
    main()
