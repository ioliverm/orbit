//! Canonical autonomía list served by `GET /api/v1/residency/autonomias`.
//!
//! Source of truth for AC-4.1.2 and ADR-014 "Hardcoding the autonomía list
//! client-side — rejected". Territorio común autonomías + Ceuta + Melilla
//! (foral = false) and País Vasco + Navarra (foral = true). The frontend
//! renders these in ES-alphabetical order; this list is already sorted.
//!
//! The `foral` flag is a visibility cue for the UI only (AC-4.1.2 shows the
//! "no soportado en v1" suffix). The handler does not reject foral
//! selections — ADR-014 §3 is explicit that they are stored but produce no
//! tax-calc block in Slice 1.

/// A single autonomía entry. Fields are kept small on purpose; the
/// frontend does not need tax metadata in Slice 1.
#[derive(Debug, Clone, Copy)]
pub struct Autonomia {
    /// ISO 3166-2 code, e.g. `ES-MD`.
    pub code: &'static str,
    /// Spanish display name.
    pub name_es: &'static str,
    /// English display name (fallback locale per AC §2).
    pub name_en: &'static str,
    /// `true` for País Vasco and Navarra (foral regime), `false` otherwise.
    pub foral: bool,
}

/// Canonical list. ES-alphabetical, with País Vasco + Navarra at the end
/// per the UX reference `residency-setup.html`.
pub const AUTONOMIAS: &[Autonomia] = &[
    Autonomia {
        code: "ES-AN",
        name_es: "Andalucía",
        name_en: "Andalusia",
        foral: false,
    },
    Autonomia {
        code: "ES-AR",
        name_es: "Aragón",
        name_en: "Aragon",
        foral: false,
    },
    Autonomia {
        code: "ES-AS",
        name_es: "Asturias",
        name_en: "Asturias",
        foral: false,
    },
    Autonomia {
        code: "ES-IB",
        name_es: "Baleares",
        name_en: "Balearic Islands",
        foral: false,
    },
    Autonomia {
        code: "ES-CN",
        name_es: "Canarias",
        name_en: "Canary Islands",
        foral: false,
    },
    Autonomia {
        code: "ES-CB",
        name_es: "Cantabria",
        name_en: "Cantabria",
        foral: false,
    },
    Autonomia {
        code: "ES-CM",
        name_es: "Castilla-La Mancha",
        name_en: "Castilla-La Mancha",
        foral: false,
    },
    Autonomia {
        code: "ES-CL",
        name_es: "Castilla y León",
        name_en: "Castile and León",
        foral: false,
    },
    Autonomia {
        code: "ES-CT",
        name_es: "Cataluña",
        name_en: "Catalonia",
        foral: false,
    },
    Autonomia {
        code: "ES-CE",
        name_es: "Ceuta",
        name_en: "Ceuta",
        foral: false,
    },
    Autonomia {
        code: "ES-EX",
        name_es: "Extremadura",
        name_en: "Extremadura",
        foral: false,
    },
    Autonomia {
        code: "ES-GA",
        name_es: "Galicia",
        name_en: "Galicia",
        foral: false,
    },
    Autonomia {
        code: "ES-RI",
        name_es: "La Rioja",
        name_en: "La Rioja",
        foral: false,
    },
    Autonomia {
        code: "ES-MD",
        name_es: "Comunidad de Madrid",
        name_en: "Madrid",
        foral: false,
    },
    Autonomia {
        code: "ES-ML",
        name_es: "Melilla",
        name_en: "Melilla",
        foral: false,
    },
    Autonomia {
        code: "ES-MC",
        name_es: "Murcia",
        name_en: "Murcia",
        foral: false,
    },
    Autonomia {
        code: "ES-VC",
        name_es: "Comunidad Valenciana",
        name_en: "Valencia",
        foral: false,
    },
    Autonomia {
        code: "ES-NA",
        name_es: "Navarra",
        name_en: "Navarre",
        foral: true,
    },
    Autonomia {
        code: "ES-PV",
        name_es: "País Vasco",
        name_en: "Basque Country",
        foral: true,
    },
];

/// Case-sensitive membership test for input validation.
pub fn is_known(code: &str) -> bool {
    AUTONOMIAS.iter().any(|a| a.code == code)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn madrid_is_present_and_non_foral() {
        let madrid = AUTONOMIAS.iter().find(|a| a.code == "ES-MD").unwrap();
        assert_eq!(madrid.name_es, "Comunidad de Madrid");
        assert!(!madrid.foral);
    }

    #[test]
    fn foral_flag_matches_pais_vasco_and_navarra() {
        let foral: Vec<_> = AUTONOMIAS.iter().filter(|a| a.foral).collect();
        assert_eq!(foral.len(), 2);
        let codes: Vec<_> = foral.iter().map(|a| a.code).collect();
        assert!(codes.contains(&"ES-PV"));
        assert!(codes.contains(&"ES-NA"));
    }

    #[test]
    fn is_known_recognises_list_codes_only() {
        assert!(is_known("ES-MD"));
        assert!(is_known("ES-PV"));
        assert!(!is_known("ES-XX"));
        assert!(!is_known("es-md")); // case-sensitive
    }
}
