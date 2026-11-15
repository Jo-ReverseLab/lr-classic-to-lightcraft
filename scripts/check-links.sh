#!/bin/sh
# Check relative links in the tracked Markdown files.
#
# The check catches a renamed or deleted file behind a documentation link. It
# does not access the network and needs no tools beyond git, grep, sed and the
# POSIX shell.
#
# Skipped targets: absolute URLs and any other target that starts with a
# scheme (for example https:, mailto:), bare anchors ("#section") and empty
# targets. Absolute paths (leading "/") are not relative to the repository.
# Before a target is resolved, a fragment ("#...") and a query ("?...") are
# removed and the target is resolved relative to the Markdown file that
# contains it. Links inside fenced code blocks are examples, not links.
#
# Exit status: 0 when every checked link resolves, 1 otherwise.

set -u

dead=$(git ls-files '*.md' | while IFS= read -r file; do
    case $file in
        (*/*) dir=${file%/*} ;;
        (*) dir=. ;;
    esac

    # Drop fenced code blocks: `](...)` inside them is an example.
    sed -n '/^```/,/^```/!p' "$file" |
        grep -oE '\]\([^)]*\)' |
        sed 's/^](//; s/)$//' |
        while IFS= read -r raw; do
            target=$raw

            # Drop an optional Markdown title (a quoted string after the
            # target) and the whitespace between the two.
            case $target in
                (*' "'*) target=${target%%\"*} ;;
                (*" '"*) target=${target%%\'*} ;;
            esac
            while [ "$target" != "${target%[[:space:]]}" ]; do
                target=${target%?}
            done

            # Angle-bracket form, used for targets that contain spaces.
            case $target in
                ('<'*'>') target=${target#'<'}
                         target=${target%'>'} ;;
            esac

            case $target in (*'#'*) target=${target%%'#'*} ;; esac
            case $target in (*'?'*) target=${target%%'?'*} ;; esac

            case $target in
                (''|'#'*|*:*|/*) continue ;;
            esac

            if [ ! -e "$dir/$target" ]; then
                printf '%s: dead link: %s\n' "$file" "$raw"
            fi
        done
done)

if [ -n "$dead" ]; then
    printf '%s\n' "$dead" >&2
    exit 1
fi
