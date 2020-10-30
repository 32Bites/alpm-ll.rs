use crate::{
    free, Alpm, Backup, Conflict, Db, DbMut, Dep, DepMissing, Depend, DependMissing, FileConflict,
    Group, LoadedPackage, OwnedConflict, OwnedFileConflict, Package,
};

use std::ffi::{c_void, CStr};
use std::iter::{ExactSizeIterator, Iterator};
use std::marker::PhantomData;
use std::os::raw::c_char;
use std::ptr;

use alpm_sys::*;

extern "C" {
    fn strndup(cs: *const c_char, n: usize) -> *mut c_char;
}

pub unsafe trait IntoAlpmListItem<'a, 'b> {
    type Borrow;
    unsafe fn into_alpm_list_item(handle: &'a Alpm, ptr: *mut c_void) -> Self;
    unsafe fn as_alpm_list_item(handle: &'a Alpm, ptr: *mut c_void) -> Self::Borrow;
}

pub unsafe trait AsAlpmListItemPtr<'a> {
    type Output;
    const FREE: Option<unsafe extern "C" fn(_ptr: *mut c_void)> = None;
    fn as_ptr(&self) -> *mut c_void;
}

pub trait Bool {
    const DROP: bool;
}

pub unsafe trait Push<'a>: AsAlpmListItemPtr<'a> {}

pub struct True;
pub struct False;

impl Bool for True {
    const DROP: bool = true;
}

impl Bool for False {
    const DROP: bool = false;
}

pub struct RawAlpmList<'a, T, D>
where
    D: Bool,
    T: AsAlpmListItemPtr<'a>,
{
    list: *mut alpm_list_t,
    _marker1: PhantomData<&'a T>,
    _marker2: PhantomData<D>,
}

impl<'a, T, D> RawAlpmList<'a, T, D>
where
    D: Bool,
    T: AsAlpmListItemPtr<'a>,
{
    pub fn list(&self) -> *mut alpm_list_t {
        self.list
    }
}

impl<'a, T, D> Drop for RawAlpmList<'a, T, D>
where
    D: Bool,
    T: AsAlpmListItemPtr<'a>,
{
    fn drop(&mut self) {
        if D::DROP {
            if let Some(free) = T::FREE {
                unsafe { alpm_list_free_inner(self.list, Some(free)) }
            }
            unsafe { alpm_list_free(self.list) };
        }
    }
}

pub trait AsRawAlpmList<'a, T>
where
    T: AsAlpmListItemPtr<'a>,
{
    type Drop: Bool;
    unsafe fn as_raw_alpm_list(self) -> RawAlpmList<'a, T, Self::Drop>;
}

impl<'a> AsRawAlpmList<'a, Package<'a>> for AlpmList<'a, LoadedPackage<'a>> {
    type Drop = False;
    unsafe fn as_raw_alpm_list(self) -> RawAlpmList<'a, Package<'a>, Self::Drop> {
        RawAlpmList {
            list: self.list,
            _marker1: PhantomData,
            _marker2: PhantomData,
        }
    }
}

impl<'a, T> AsRawAlpmList<'a, T> for AlpmList<'a, T>
where
    T: AsAlpmListItemPtr<'a>,
{
    type Drop = False;
    unsafe fn as_raw_alpm_list(self) -> RawAlpmList<'a, T, Self::Drop> {
        RawAlpmList {
            list: self.list,
            _marker1: PhantomData,
            _marker2: PhantomData,
        }
    }
}

impl<'a, T> AsRawAlpmList<'a, T> for &AlpmListMut<'a, T>
where
    for<'b> T: IntoAlpmListItem<'a, 'b> + AsAlpmListItemPtr<'a>,
{
    type Drop = False;
    unsafe fn as_raw_alpm_list(self) -> RawAlpmList<'a, T, Self::Drop> {
        RawAlpmList {
            list: self.list.list,
            _marker1: PhantomData,
            _marker2: PhantomData,
        }
    }
}

impl<'a, T, I> AsRawAlpmList<'a, T::Output> for I
where
    I: Iterator<Item = T>,
    T: AsAlpmListItemPtr<'a>,
    T::Output: AsAlpmListItemPtr<'a>,
{
    type Drop = True;
    unsafe fn as_raw_alpm_list(self) -> RawAlpmList<'a, T::Output, Self::Drop> {
        let mut list = ptr::null_mut();

        for item in self {
            list = alpm_list_add(list, item.as_ptr());
            if T::FREE.is_none() {
                std::mem::forget(item);
            }
        }

        RawAlpmList {
            list,
            _marker1: PhantomData,
            _marker2: PhantomData,
        }
    }
}

#[derive(Debug)]
pub struct AlpmList<'a, T> {
    pub(crate) handle: &'a Alpm,
    pub(crate) list: *mut alpm_list_t,
    pub(crate) _marker: PhantomData<T>,
}

impl<'a, T> Clone for AlpmList<'a, T> {
    fn clone(&self) -> Self {
        AlpmList {
            handle: self.handle,
            list: self.list,
            _marker: self._marker,
        }
    }
}

impl<'a, T> Copy for AlpmList<'a, T> {}

#[derive(Debug)]
pub struct AlpmListMut<'a, T>
where
    for<'b> T: IntoAlpmListItem<'a, 'b>,
{
    list: AlpmList<'a, T>,
}

impl<'a, T> std::ops::Deref for AlpmListMut<'a, T>
where
    for<'b> T: IntoAlpmListItem<'a, 'b>,
{
    type Target = AlpmList<'a, T>;

    fn deref(&self) -> &Self::Target {
        &self.list
    }
}

impl<'a, T> Drop for AlpmListMut<'a, T>
where
    for<'b> T: IntoAlpmListItem<'a, 'b>,
{
    fn drop(&mut self) {
        let list = self.list.list;
        let mut curr = list;

        while !curr.is_null() {
            let item = unsafe { T::into_alpm_list_item(self.handle, (*curr).data) };
            drop(item);
            curr = unsafe { (*curr).next };
        }

        unsafe { alpm_list_free(list) }
    }
}

impl<'a, 'b, T> AlpmList<'a, T>
where
    T: IntoAlpmListItem<'a, 'b>,
{
    pub fn len(&self) -> usize {
        unsafe { alpm_list_count(self.list) }
    }

    pub fn is_empty(&self) -> bool {
        self.list.is_null()
    }

    pub fn first(&'b self) -> Option<T::Borrow> {
        if self.is_empty() {
            None
        } else {
            unsafe { Some(T::as_alpm_list_item(self.handle, (*self.list).data)) }
        }
    }

    pub fn last(&'b self) -> Option<T::Borrow> {
        let item = unsafe { alpm_list_last(self.list) };
        if item.is_null() {
            None
        } else {
            unsafe { Some(T::as_alpm_list_item(self.handle, (*item).data)) }
        }
    }

    pub fn iter(&'b self) -> Iter<'a, 'b, T> {
        self.into_iter()
    }
}

impl<'a> AlpmList<'a, String> {
    pub fn as_str<'b>(&'b self) -> AlpmList<'a, &'b str> {
        AlpmList::from_parts(self.handle, self.list)
    }
}

impl<'a, T> AlpmList<'a, T>
where
    for<'b> T: IntoAlpmListItem<'a, 'b>,
{
    pub fn to_list(&self) -> AlpmListMut<'a, T> {
        let list = unsafe { alpm_list_copy(self.list) };
        AlpmListMut {
            list: AlpmList::from_parts(self.handle, list),
        }
    }
}

impl<'a, T> AlpmListMut<'a, T>
where
    for<'b> T: IntoAlpmListItem<'a, 'b> + Push<'a> + AsAlpmListItemPtr<'a>,
{
    pub fn push(&mut self, t: T) {
        unsafe { self.list.list = alpm_list_add(self.list.list, t.as_ptr()) };
        if T::FREE.is_none() {
            std::mem::forget(t);
        }
    }
}

impl<'a, T> Extend<T> for AlpmListMut<'a, T>
where
    for<'b> T: IntoAlpmListItem<'a, 'b> + Push<'a>,
{
    fn extend<I: IntoIterator<Item = T>>(&mut self, iter: I) {
        for item in iter {
            self.push(item);
        }
    }
}

impl<'a> AlpmListMut<'a, String> {
    pub fn push_str(&mut self, s: &str) {
        let s = unsafe { strndup(s.as_bytes().as_ptr() as _, s.len()) };
        unsafe { self.list.list = alpm_list_add(self.list.list, s as *mut c_void) };
    }
}

impl<'a, T> AlpmListMut<'a, T>
where
    for<'b> T: IntoAlpmListItem<'a, 'b>,
{
    pub fn retain<F>(&mut self, mut f: F)
    where
        F: FnMut(&T) -> bool,
    {
        let mut list = self.list.list;
        let mut curr = list;

        while !curr.is_null() {
            let item = unsafe { T::into_alpm_list_item(self.handle, (*curr).data) };
            let next = unsafe { (*curr).next };
            if !f(&item) {
                drop(item);
                unsafe { list = alpm_list_remove_item(list, curr) };
                unsafe { free(curr as _) };
            } else {
                std::mem::forget(item);
            }
            curr = next;
        }

        self.list.list = list;
    }

    pub fn remove(&mut self, n: usize) -> Option<T> {
        if n >= self.len() {
            return None;
        }

        let item = unsafe { alpm_list_nth(self.list.list, n) };
        unsafe { self.list.list = alpm_list_remove_item(self.list.list, item) };
        let ret = unsafe { Some(T::into_alpm_list_item(self.handle, (*self.list.list).data)) };
        unsafe { alpm_list_free(item) };
        ret
    }

    pub fn remove_list(&mut self, n: usize) -> AlpmListMut<'a, T> {
        if n >= self.len() {
            return AlpmListMut::new(self.handle);
        }

        let item = unsafe { alpm_list_nth(self.list.list, n) };
        self.list.list = unsafe { alpm_list_remove_item(self.list.list, item) };
        AlpmListMut::from_parts(self.handle, item)
    }

    pub fn as_alpm_list(&self) -> AlpmList<'a, T> {
        self.list
    }
}

impl<'a, T> IntoIterator for AlpmListMut<'a, T>
where
    for<'b> T: IntoAlpmListItem<'a, 'b>,
{
    type Item = T;
    type IntoIter = IntoIterMut<'a, T>;

    fn into_iter(self) -> Self::IntoIter {
        IntoIterMut {
            current: self.list.list,
            list: self,
        }
    }
}

impl<'a, 'b, T> IntoIterator for &'b AlpmListMut<'a, T>
where
    for<'c> T: IntoAlpmListItem<'a, 'c>,
{
    type Item = <T as IntoAlpmListItem<'a, 'b>>::Borrow;
    type IntoIter = Iter<'a, 'b, T>;

    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

impl<'a, 'b, T> IntoIterator for &'b AlpmList<'a, T>
where
    T: IntoAlpmListItem<'a, 'b>,
{
    type Item = T::Borrow;
    type IntoIter = Iter<'a, 'b, T>;

    fn into_iter(self) -> Self::IntoIter {
        Iter {
            current: self.list,
            list: self,
        }
    }
}

impl<'a, T> IntoIterator for AlpmList<'a, T>
where
    T: IntoAlpmListItem<'a, 'a>,
{
    type Item = T::Borrow;
    type IntoIter = IntoIter<'a, T>;

    fn into_iter(self) -> Self::IntoIter {
        IntoIter {
            current: self.list,
            list: self,
        }
    }
}

#[derive(Debug, Copy, Clone)]
pub struct Iter<'a, 'b, T>
where
    T: IntoAlpmListItem<'a, 'b>,
{
    list: &'b AlpmList<'a, T>,
    current: *mut alpm_list_t,
}

#[derive(Debug, Copy, Clone)]
pub struct IntoIter<'a, T>
where
    T: IntoAlpmListItem<'a, 'a>,
{
    list: AlpmList<'a, T>,
    current: *mut alpm_list_t,
}

impl<'a, 'b, T> Iter<'a, 'b, T>
where
    T: IntoAlpmListItem<'a, 'b>,
{
    fn next_data(&mut self) -> Option<*mut c_void> {
        if self.current.is_null() {
            None
        } else {
            let data = unsafe { (*(self.current)).data };
            self.current = unsafe { alpm_list_next(self.current) };

            Some(data)
        }
    }
}

#[derive(Debug)]
pub struct IntoIterMut<'a, T>
where
    for<'b> T: IntoAlpmListItem<'a, 'b>,
{
    list: AlpmListMut<'a, T>,
    current: *mut alpm_list_t,
}

impl<'a, T> ExactSizeIterator for IntoIterMut<'a, T> where for<'b> T: IntoAlpmListItem<'a, 'b> {}
impl<'a, T> ExactSizeIterator for IntoIter<'a, T> where for<'b> T: IntoAlpmListItem<'a, 'b> {}
impl<'a, 'b, T> ExactSizeIterator for Iter<'a, 'b, T> where T: IntoAlpmListItem<'a, 'b> {}

impl<'a, T> Iterator for IntoIterMut<'a, T>
where
    for<'b> T: IntoAlpmListItem<'a, 'b>,
{
    type Item = T;
    fn next(&mut self) -> Option<Self::Item> {
        let data = self.next_data();

        match data {
            Some(data) => unsafe { Some(T::into_alpm_list_item(self.list.handle, data)) },
            None => None,
        }
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let size = unsafe { alpm_list_count(self.list.list.list) };
        (size, Some(size))
    }
}

impl<'a, 'b, T> Iterator for Iter<'a, 'b, T>
where
    T: IntoAlpmListItem<'a, 'b>,
{
    type Item = T::Borrow;
    fn next(&mut self) -> Option<Self::Item> {
        let data = self.next_data();

        match data {
            Some(data) => unsafe { Some(T::as_alpm_list_item(self.list.handle, data)) },
            None => None,
        }
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let size = unsafe { alpm_list_count(self.list.list) };
        (size, Some(size))
    }
}

impl<'a, T> Iterator for IntoIter<'a, T>
where
    T: IntoAlpmListItem<'a, 'a>,
{
    type Item = T::Borrow;
    fn next(&mut self) -> Option<Self::Item> {
        let data = self.next_data();

        match data {
            Some(data) => unsafe { Some(T::as_alpm_list_item(self.list.handle, data)) },
            None => None,
        }
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let size = unsafe { alpm_list_count(self.list.list) };
        (size, Some(size))
    }
}

impl<'a, T> IntoIter<'a, T>
where
    T: IntoAlpmListItem<'a, 'a>,
{
    fn next_data(&mut self) -> Option<*mut c_void> {
        if self.current.is_null() {
            None
        } else {
            let data = unsafe { (*(self.current)).data };
            self.current = unsafe { alpm_list_next(self.current) };

            Some(data)
        }
    }
}

impl<'a, T> IntoIterMut<'a, T>
where
    for<'b> T: IntoAlpmListItem<'a, 'b>,
{
    fn next_data(&mut self) -> Option<*mut c_void> {
        if self.current.is_null() {
            None
        } else {
            let data = unsafe { (*(self.current)).data };
            self.current = unsafe { alpm_list_next(self.current) };

            Some(data)
        }
    }
}

impl<'a, T> AlpmList<'a, T> {
    pub(crate) fn from_parts(handle: &'a Alpm, list: *mut alpm_list_t) -> AlpmList<'a, T> {
        AlpmList {
            handle,
            list,
            _marker: PhantomData,
        }
    }
}

impl<'a, T> AlpmListMut<'a, T>
where
    for<'b> T: IntoAlpmListItem<'a, 'b>,
{
    pub fn new(handle: &'a Alpm) -> AlpmListMut<'a, T> {
        AlpmListMut {
            list: AlpmList::from_parts(handle, ptr::null_mut()),
        }
    }

    pub(crate) fn from_parts(handle: &'a Alpm, list: *mut alpm_list_t) -> AlpmListMut<'a, T> {
        AlpmListMut {
            list: AlpmList::from_parts(handle, list),
        }
    }
}

unsafe impl<'a> AsAlpmListItemPtr<'a> for Package<'a> {
    type Output = Package<'a>;

    fn as_ptr(&self) -> *mut c_void {
        self.pkg as *mut c_void
    }
}

unsafe impl<'a> AsAlpmListItemPtr<'a> for &Package<'a> {
    type Output = Package<'a>;

    fn as_ptr(&self) -> *mut c_void {
        self.pkg as *mut c_void
    }
}

unsafe impl<'a> AsAlpmListItemPtr<'a> for Db<'a> {
    type Output = Db<'a>;

    fn as_ptr(&self) -> *mut c_void {
        self.db as *mut c_void
    }
}

unsafe impl<'a> AsAlpmListItemPtr<'a> for &Db<'a> {
    type Output = Db<'a>;

    fn as_ptr(&self) -> *mut c_void {
        self.db as *mut c_void
    }
}

unsafe impl<'a> AsAlpmListItemPtr<'a> for Depend {
    type Output = Dep<'a>;

    fn as_ptr(&self) -> *mut c_void {
        self.inner as *mut c_void
    }
}

unsafe impl<'a> AsAlpmListItemPtr<'a> for Dep<'a> {
    type Output = Dep<'a>;

    fn as_ptr(&self) -> *mut c_void {
        self.inner as *mut c_void
    }
}

unsafe impl<'a> AsAlpmListItemPtr<'a> for String {
    type Output = String;
    const FREE: Option<unsafe extern "C" fn(_ptr: *mut c_void)> = Some(free);
    fn as_ptr(&self) -> *mut c_void {
        unsafe { strndup(self.as_bytes().as_ptr() as _, self.len()) as *mut c_void }
    }
}

unsafe impl<'a> AsAlpmListItemPtr<'a> for &String {
    type Output = String;
    const FREE: Option<unsafe extern "C" fn(_ptr: *mut c_void)> = Some(free);

    fn as_ptr(&self) -> *mut c_void {
        unsafe { strndup(self.as_bytes().as_ptr() as _, self.len()) as *mut c_void }
    }
}

unsafe impl<'a> AsAlpmListItemPtr<'a> for &str {
    type Output = String;
    const FREE: Option<unsafe extern "C" fn(_ptr: *mut c_void)> = Some(free);

    fn as_ptr(&self) -> *mut c_void {
        unsafe { strndup(self.as_bytes().as_ptr() as _, self.len()) as *mut c_void }
    }
}

unsafe impl<'a> Push<'a> for String {}
unsafe impl<'a> Push<'a> for Package<'a> {}
unsafe impl<'a> Push<'a> for Db<'a> {}
unsafe impl<'a> Push<'a> for Depend {}
unsafe impl<'a> Push<'a> for Dep<'a> {}

unsafe impl<'a, 'b> IntoAlpmListItem<'a, 'b> for Package<'a> {
    type Borrow = Self;
    unsafe fn into_alpm_list_item(handle: &'a Alpm, ptr: *mut c_void) -> Self {
        Package::new(handle, ptr as *mut alpm_pkg_t)
    }
    unsafe fn as_alpm_list_item(handle: &'a Alpm, ptr: *mut c_void) -> Self::Borrow {
        Package::new(handle, ptr as *mut alpm_pkg_t)
    }
}

unsafe impl<'a, 'b> IntoAlpmListItem<'a, 'b> for Group<'a> {
    type Borrow = Self;
    unsafe fn into_alpm_list_item(handle: &'a Alpm, ptr: *mut c_void) -> Self {
        Group {
            inner: ptr as *mut alpm_group_t,
            handle,
        }
    }
    unsafe fn as_alpm_list_item(handle: &'a Alpm, ptr: *mut c_void) -> Self::Borrow {
        Group {
            inner: ptr as *mut alpm_group_t,
            handle,
        }
    }
}

unsafe impl<'a, 'b> IntoAlpmListItem<'a, 'b> for Depend {
    type Borrow = Dep<'b>;
    unsafe fn into_alpm_list_item(_handle: &'a Alpm, ptr: *mut c_void) -> Self {
        Depend::from_ptr(ptr as *mut alpm_depend_t)
    }
    unsafe fn as_alpm_list_item(_handle: &'a Alpm, ptr: *mut c_void) -> Self::Borrow {
        Dep::from_ptr(ptr as *mut alpm_depend_t)
    }
}

unsafe impl<'a, 'b> IntoAlpmListItem<'a, 'b> for Dep<'a> {
    type Borrow = Self;
    unsafe fn into_alpm_list_item(_handle: &'a Alpm, ptr: *mut c_void) -> Self {
        Dep::from_ptr(ptr as *mut alpm_depend_t)
    }

    unsafe fn as_alpm_list_item(_handle: &'a Alpm, ptr: *mut c_void) -> Self::Borrow {
        Dep::from_ptr(ptr as *mut alpm_depend_t)
    }
}

unsafe impl<'a, 'b> IntoAlpmListItem<'a, 'b> for Backup {
    type Borrow = Self;
    unsafe fn into_alpm_list_item(_handle: &'a Alpm, ptr: *mut c_void) -> Self {
        Backup {
            inner: ptr as *mut alpm_backup_t,
        }
    }
    unsafe fn as_alpm_list_item(_handle: &'a Alpm, ptr: *mut c_void) -> Self::Borrow {
        Backup {
            inner: ptr as *mut alpm_backup_t,
        }
    }
}

unsafe impl<'a, 'b> IntoAlpmListItem<'a, 'b> for OwnedFileConflict {
    type Borrow = FileConflict<'b>;
    unsafe fn into_alpm_list_item(_handle: &'a Alpm, ptr: *mut c_void) -> Self {
        OwnedFileConflict {
            inner: FileConflict {
                inner: ptr as *mut alpm_fileconflict_t,
                phantom: PhantomData,
            },
        }
    }

    unsafe fn as_alpm_list_item(_handle: &'a Alpm, ptr: *mut c_void) -> Self::Borrow {
        FileConflict {
            inner: ptr as *mut alpm_fileconflict_t,
            phantom: PhantomData,
        }
    }
}

unsafe impl<'a, 'b> IntoAlpmListItem<'a, 'b> for DependMissing {
    type Borrow = DepMissing<'b>;
    unsafe fn into_alpm_list_item(_handle: &'a Alpm, ptr: *mut c_void) -> Self {
        DependMissing {
            inner: DepMissing {
                inner: ptr as *mut alpm_depmissing_t,
                phantom: PhantomData,
            },
        }
    }

    unsafe fn as_alpm_list_item(_handle: &'a Alpm, ptr: *mut c_void) -> Self::Borrow {
        DepMissing {
            inner: ptr as *mut alpm_depmissing_t,
            phantom: PhantomData,
        }
    }
}

unsafe impl<'a, 'b> IntoAlpmListItem<'a, 'b> for OwnedConflict {
    type Borrow = Conflict<'b>;
    unsafe fn into_alpm_list_item(_handle: &'a Alpm, ptr: *mut c_void) -> Self {
        OwnedConflict::from_ptr(ptr as *mut alpm_conflict_t)
    }
    unsafe fn as_alpm_list_item(_handle: &'a Alpm, ptr: *mut c_void) -> Self::Borrow {
        Conflict::from_ptr(ptr as *mut alpm_conflict_t)
    }
}

unsafe impl<'a, 'b> IntoAlpmListItem<'a, 'b> for Conflict<'a> {
    type Borrow = Self;
    unsafe fn into_alpm_list_item(_handle: &'a Alpm, ptr: *mut c_void) -> Self {
        Conflict::from_ptr(ptr as *mut alpm_conflict_t)
    }
    unsafe fn as_alpm_list_item(_handle: &'a Alpm, ptr: *mut c_void) -> Self::Borrow {
        Conflict::from_ptr(ptr as *mut alpm_conflict_t)
    }
}

unsafe impl<'a, 'b> IntoAlpmListItem<'a, 'b> for Db<'a> {
    type Borrow = Self;
    unsafe fn into_alpm_list_item(handle: &'a Alpm, ptr: *mut c_void) -> Self {
        Db {
            db: ptr as *mut alpm_db_t,
            handle,
        }
    }
    unsafe fn as_alpm_list_item(handle: &'a Alpm, ptr: *mut c_void) -> Self::Borrow {
        Db {
            db: ptr as *mut alpm_db_t,
            handle,
        }
    }
}

unsafe impl<'a, 'b> IntoAlpmListItem<'a, 'b> for DbMut<'a> {
    type Borrow = Self;
    unsafe fn into_alpm_list_item(handle: &'a Alpm, ptr: *mut c_void) -> Self {
        DbMut {
            inner: Db {
                db: ptr as *mut alpm_db_t,
                handle,
            },
        }
    }
    unsafe fn as_alpm_list_item(handle: &'a Alpm, ptr: *mut c_void) -> Self::Borrow {
        DbMut {
            inner: Db {
                db: ptr as *mut alpm_db_t,
                handle,
            },
        }
    }
}

unsafe impl<'a, 'b> IntoAlpmListItem<'a, 'b> for &'a str {
    type Borrow = Self;
    unsafe fn into_alpm_list_item(_handle: &'a Alpm, ptr: *mut c_void) -> Self {
        let s = CStr::from_ptr(ptr as *mut c_char);
        s.to_str().unwrap()
    }
    unsafe fn as_alpm_list_item(_handle: &'a Alpm, ptr: *mut c_void) -> Self::Borrow {
        let s = CStr::from_ptr(ptr as *mut c_char);
        s.to_str().unwrap()
    }
}

unsafe impl<'a, 'b> IntoAlpmListItem<'a, 'b> for String {
    type Borrow = &'b str;
    unsafe fn into_alpm_list_item(_handle: &'a Alpm, ptr: *mut c_void) -> Self {
        let s = CStr::from_ptr(ptr as *mut c_char);
        let s = s.to_str().unwrap().to_string();
        free(ptr);
        s
    }
    unsafe fn as_alpm_list_item(_handle: &'a Alpm, ptr: *mut c_void) -> Self::Borrow {
        let s = CStr::from_ptr(ptr as *mut c_char);
        s.to_str().unwrap()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::SigLevel;

    #[test]
    fn test_depends_list_free() {
        let handle = Alpm::new("/", "tests/db").unwrap();
        let db = handle.register_syncdb("core", SigLevel::NONE).unwrap();
        let pkg = db.pkg("linux").unwrap();
        let depends = pkg.depends();
        assert_eq!(depends.first().unwrap().to_string(), "coreutils");
    }

    #[test]
    fn test_is_empty() {
        let handle = Alpm::new("/", "tests/db").unwrap();
        let db = handle.register_syncdb("core", SigLevel::NONE).unwrap();
        let pkg = db.pkg("linux").unwrap();
        let depends = pkg.depends();
        assert!(!depends.is_empty());

        let pkg = db.pkg("tzdata").unwrap();
        let depends = pkg.depends();
        assert!(depends.is_empty());
    }

    #[test]
    fn test_string_list_free() {
        let handle = Alpm::new("/", "tests/db").unwrap();
        handle.register_syncdb("community", SigLevel::NONE).unwrap();
        handle.register_syncdb("extra", SigLevel::NONE).unwrap();
        let db = handle.register_syncdb("core", SigLevel::NONE).unwrap();
        let pkg = db.pkg("linux").unwrap();
        let required_by = pkg.required_by();
        assert_eq!("acpi_call", required_by.first().unwrap());
    }

    #[test]
    fn test_str_list_free() {
        let handle = Alpm::new("/", "tests/db").unwrap();
        let db = handle.register_syncdb("core", SigLevel::NONE).unwrap();
        let pkg = db.pkg("pacman").unwrap();
        let groups = pkg.groups();
        assert_eq!("base", groups.first().unwrap());
    }

    #[test]
    fn test_push() {
        let handle = Alpm::new("/", "tests/db").unwrap();
        let db = handle.register_syncdb("core", SigLevel::NONE).unwrap();
        let pkg = db.pkg("pacman").unwrap();

        let mut list = AlpmListMut::new(&handle);
        list.push(pkg);

        assert_eq!(list.first().unwrap().name(), "pacman");
    }

    #[test]
    fn test_retain() {
        let handle = Alpm::new("/", "tests/db").unwrap();
        let db = handle.register_syncdb("core", SigLevel::NONE).unwrap();
        let mut pkgs = db.pkgs().to_list();
        pkgs.retain(|p| p.name().starts_with('a'));

        assert!(!pkgs.is_empty());
        pkgs.iter().for_each(|p| assert!(p.name().starts_with('a')));
    }

    #[test]
    fn test_as_raw_alpm_list() {
        let handle = Alpm::new("/", "tests/db").unwrap();
        let db = handle.register_syncdb("core", SigLevel::NONE).unwrap();
        let pkg = db.pkg("linux").unwrap();

        pkg.sync_new_version(handle.syncdbs());
        pkg.sync_new_version(&handle.syncdbs().to_list().remove_list(0));
        pkg.sync_new_version(vec![db].into_iter());
        pkg.sync_new_version(vec![db].iter());
    }
}
