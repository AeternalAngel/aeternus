_aeternus() {
    local cur=${COMP_WORDS[COMP_CWORD]}
    local prev=${COMP_WORDS[COMP_CWORD-1]}
    case $prev in
        -c|--config)
            COMPREPLY=($(compgen -f -- "$cur"))
            return
            ;;
    esac
    COMPREPLY=($(compgen -W "--config --bench --help --version" -- "$cur"))
}
complete -o filenames -F _aeternus aeternus
