# kintsu — fish hook. In ~/.config/fish/config.fish:   kintsu init fish | source
#
# fish_postexec fires after every command line with its text in $argv and
# the exit status in $status; empty lines never fire it.

function __kintsu_postexec --on-event fish_postexec
    set -l kintsu_status $status
    test $kintsu_status -eq 0; and return 0
    string match -q 'kintsu*' -- "$argv[1]"; and return $kintsu_status
    command kintsu triage --status $kintsu_status --command "$argv[1]"
    return $kintsu_status
end
