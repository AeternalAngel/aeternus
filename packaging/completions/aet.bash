_aet() {
    local cur=${COMP_WORDS[COMP_CWORD]}
    if [[ $COMP_CWORD -eq 1 ]]; then
        COMPREPLY=($(compgen -W "version pref reload --help --version" -- "$cur"))
    fi
}
complete -F _aet aet
