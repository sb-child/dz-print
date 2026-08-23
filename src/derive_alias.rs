use derive_aliases::define;

define! {
    #![export_derive_aliases]
    CmdTemplate = ::core::fmt::Debug, ::std::cmp::PartialEq, ::deku::DekuWrite, ::std::default::Default, ::std::clone::Clone;
}
