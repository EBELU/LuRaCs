def logo(version: str, color: bool = False, is_h3: bool = False):
    if color:
        bg_yellow = "\033[43m\033[30m"
        bg_blue = "\033[44m"
        bg_green = "\033[42m\033[30m"
        bg_red = "\033[41m\033[30m"
        reset = "\033[0m"
    else:
        bg_yellow = ""
        bg_blue = ""
        bg_green = ""
        bg_red = ""
        reset = ""

    base_message = [
        f"{bg_green} ====== {reset}{bg_blue} ====== {reset}{bg_yellow} ====== {reset}",
        f"{bg_green}|71    |{reset}{bg_blue}|88    |{reset}{bg_yellow}|55    |{reset}",
        f"{bg_green}|  Lu  |{reset}{bg_blue}|  Ra  |{reset}{bg_yellow}|  Cs  |{reset}",
        f"{bg_green}| 177  |{reset}{bg_blue}| 226  |{reset}{bg_yellow}| 137  |{reset}",
        f"{bg_green} ====== {reset}{bg_blue} ====== {reset}{bg_yellow} ====== {reset}",
    ]

    h3_message = [
        f"  {bg_red} ====== {reset}",
        f"  {bg_red}|1     |{reset}",
        f"=={bg_red}|  H   |{reset}",
        f"  {bg_red}|  3   |{reset}",
        f"  {bg_red} ====== {reset}",
    ]

    end_message = [
        "",
        f"   Version:  {version}",
        "   LuRaCs Console",
        "   Type 'help' for a list of commands",
        "",
    ]

    for i in range(len(base_message)):
        if is_h3:
            base_message[i] += h3_message[i]
        base_message[i] += end_message[i]

    return "\n".join(base_message)


if __name__ == "__main__":
    print(logo("2.2", True, True))
