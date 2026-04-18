import json
from glob import glob
from pathlib import Path
import os

pth = "dev/raw_nuclide_data"


def is_float(x):
    try:
        float(x)
        return True
    except ValueError:
        return False


os.makedirs("dev/cleaned_nuclide_data", exist_ok=True)

for file in glob(pth + "/*"):
    volume = int(Path(file).stem.split("_")[-1].removeprefix("Vol"))
    json_dict = {"lnhb_volume": volume}
    emissions = []
    with open(file) as f:
        for line in f.readlines():
            line_parts = [part.strip() for part in line.split(";")]

            match line_parts[0]:
                case "Nuclide":
                    json_dict["Nuclide"] = line_parts[1]
                case "Element":
                    json_dict["Element"] = line_parts[1]
                case "Z":
                    json_dict["Z"] = line_parts[1]

                case "Daughter(s)":
                    daughters = []
                    for i in range(1, len(line_parts), 3):
                        daughters.append([line_parts[i + 1], float(line_parts[i + 2])])
                    json_dict["Daughters"] = daughters

                case "Possible parent(s)":
                    parents = []
                    for i in range(1, len(line_parts), 3):
                        parents.append([line_parts[i + 1], float(line_parts[i + 2])])
                    json_dict["Parents"] = parents

                case "Half-life (s)":
                    json_dict["Half-life (s)"] = [
                        float(line_parts[1]),
                        float(line_parts[2]),
                    ]

                case "Specific activity (Bq/g)":
                    json_dict["Specific activity (Bq/g)"] = [
                        float(line_parts[1]),
                        float(line_parts[2]),
                    ]

                case _:
                    if "--------" in line_parts[0]:
                        continue

                    elif is_float(line_parts[0]):
                        if float(line_parts[0]) < 20 or float(line_parts[2]) < 1:
                            continue
                        emissions.append(
                            {
                                "Energy (keV)": float(line_parts[0]),
                                "Energy error (keV)": float(line_parts[1])
                                if line_parts[1]
                                else -1,
                                "I (%)": float(line_parts[2]),
                                "I error (%)": float(line_parts[3])
                                if line_parts[3]
                                else -1,
                                "Type": "g" if line_parts[4] == "g" else "x-ray",
                                "Origin": line_parts[5],
                            }
                        )

    json_dict["Emissions"] = emissions

    with open(
        (Path("dev/cleaned_nuclide_data") / Path(file).stem.split("_")[0]).with_suffix(
            ".json"
        ),
        "w",
    ) as f:
        json.dump(json_dict, f, indent=4, ensure_ascii=False)
